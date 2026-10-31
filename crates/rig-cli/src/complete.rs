//! Inline completion for `/commands`, `@files`, `$skills` and command arguments.
//!
//! `detect` looks at the text and cursor and says what is being completed; `build`
//! produces ranked candidates. Both are pure so they can be tested without a terminal.

use rig_core::commands::{self, Cmd};
use rig_core::fuzzy::rank_label;
use rig_core::permission::Autonomy;
use rig_core::provider::{Effort, ModelInfo};
use rig_core::skills::Skill;
use rig_tools::index::Index;

pub const MAX_ROWS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// `/que` at the start of the line.
    Slash { query: String },
    /// Text after `/cmd `.
    Arg {
        cmd: Cmd,
        query: String,
        start: usize,
    },
    /// `@que`; `start` is the sigil's char index.
    At { query: String, start: usize },
    /// `$que`.
    Dollar { query: String, start: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Command,
    File,
    Dir,
    Skill,
    Value,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub label: String,
    pub detail: String,
    /// What replaces the completed token.
    pub insert: String,
    /// Char positions in `label` to highlight.
    pub positions: Vec<usize>,
    pub kind: Kind,
    /// Selecting this command needs no argument, so Enter can run it.
    pub complete_command: bool,
}

pub struct Completion {
    pub trigger: Trigger,
    pub items: Vec<Item>,
    pub selected: usize,
    /// Char range of the text the accepted item replaces.
    pub range: (usize, usize),
    pub loading: bool,
}

impl Completion {
    pub fn selected_item(&self) -> Option<&Item> {
        self.items.get(self.selected)
    }

    pub fn nav(&mut self, delta: i32) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len() as i32;
        self.selected = (((self.selected as i32 + delta) % n + n) % n) as usize;
    }
}

fn starts_token(prev: Option<char>) -> bool {
    prev.is_none_or(|p| p.is_whitespace() || matches!(p, '(' | '[' | '{' | '<' | '\'' | '"' | '`'))
}

/// Commands whose argument has completions.
fn takes_arg_completion(c: Cmd) -> bool {
    matches!(c, Cmd::Model | Cmd::Effort | Cmd::Permissions | Cmd::Skills)
}

/// Commands that open their own filterable picker when run bare. They get no second popup for an
/// empty argument: one selector at a time.
fn opens_picker(c: Cmd) -> bool {
    matches!(
        c,
        Cmd::Model | Cmd::Effort | Cmd::Permissions | Cmd::Settings | Cmd::Profile
    )
}

pub fn detect(text: &[char], cursor: usize) -> Option<Trigger> {
    let before = &text[..cursor.min(text.len())];
    if before.first() == Some(&'/') {
        match before.iter().position(|c| c.is_whitespace()) {
            None => {
                return Some(Trigger::Slash {
                    query: before[1..].iter().collect(),
                });
            }
            Some(sp) if !before.contains(&'\n') => {
                let name: String = before[1..sp].iter().collect();
                let spec = commands::lookup(&name)?;
                if takes_arg_completion(spec.cmd) {
                    let query = before[sp + 1..]
                        .iter()
                        .collect::<String>()
                        .trim_start()
                        .to_string();
                    if query.is_empty() && opens_picker(spec.cmd) {
                        return None;
                    }
                    return Some(Trigger::Arg {
                        cmd: spec.cmd,
                        query,
                        start: sp + 1,
                    });
                }
                return None;
            }
            Some(_) => {}
        }
    }
    // Walk back over the current word looking for a sigil that begins a token.
    let mut i = before.len();
    while i > 0 && !before[i - 1].is_whitespace() {
        i -= 1;
        let c = before[i];
        if (c == '@' || c == '$') && starts_token(i.checked_sub(1).map(|j| before[j])) {
            let query: String = before[i + 1..].iter().collect();
            let query = query.trim_start_matches('"').to_string();
            return Some(if c == '@' {
                Trigger::At { query, start: i }
            } else {
                Trigger::Dollar { query, start: i }
            });
        }
    }
    None
}

pub struct Sources<'a> {
    pub index: &'a Index,
    pub skills: &'a [Skill],
    pub models: &'a [ModelInfo],
    pub efforts: &'a [Effort],
    pub current_model: &'a str,
}

fn quote_if_needed(path: &str) -> String {
    if path
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '(' | ')'))
    {
        format!("\"{path}\"")
    } else {
        path.to_string()
    }
}

pub fn build(trigger: Trigger, cursor: usize, src: &Sources) -> Completion {
    let (items, range, loading) = match &trigger {
        Trigger::Slash { query } => {
            let items = commands::complete(query)
                .into_iter()
                .map(|c| Item {
                    label: format!("/{}", c.spec.name),
                    detail: c.spec.summary.to_string(),
                    insert: format!("/{}{}", c.spec.name, if c.spec.has_args { " " } else { "" }),
                    positions: c.m.positions.iter().map(|p| p + 1).collect(),
                    kind: Kind::Command,
                    complete_command: !c.spec.has_args || opens_picker(c.spec.cmd),
                })
                .collect();
            (items, (0, cursor), false)
        }
        Trigger::At { query, start } => {
            let hits = src.index.search(query, 60);
            let loading =
                !src.index.ready() && hits.is_empty() && !query.starts_with(['~', '/', '.']);
            let items = hits
                .into_iter()
                .map(|h| {
                    let shown = h.path.clone();
                    Item {
                        insert: format!("@{}", quote_if_needed(&h.path)),
                        positions: h.positions,
                        kind: if h.is_dir { Kind::Dir } else { Kind::File },
                        detail: String::new(),
                        label: shown,
                        complete_command: false,
                    }
                })
                .collect();
            (items, (*start, cursor), loading)
        }
        Trigger::Dollar { query, start } => {
            let mut scored: Vec<(i32, &Skill, Vec<usize>)> = src
                .skills
                .iter()
                .filter_map(|s| rank_label(query, &s.name).map(|m| (m.score, s, m.positions)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
            let items = scored
                .into_iter()
                .map(|(_, s, positions)| Item {
                    label: format!("${}", s.name),
                    detail: s.description.clone(),
                    insert: format!("${}", s.name),
                    positions: positions.into_iter().map(|p| p + 1).collect(),
                    kind: Kind::Skill,
                    complete_command: false,
                })
                .collect();
            (items, (*start, cursor), false)
        }
        Trigger::Arg { cmd, query, start } => {
            (arg_items(*cmd, query, src), (*start, cursor), false)
        }
    };
    Completion {
        trigger,
        items,
        selected: 0,
        range,
        loading,
    }
}

fn arg_items(cmd: Cmd, query: &str, src: &Sources) -> Vec<Item> {
    let mut out: Vec<(i32, Item)> = Vec::new();
    let mut add = |label: String, detail: String, insert: String, key: &str| {
        if let Some(m) = rank_label(query, key) {
            out.push((
                m.score,
                Item {
                    positions: if query.is_empty() {
                        vec![]
                    } else {
                        m.positions
                    },
                    label,
                    detail,
                    insert,
                    kind: Kind::Value,
                    complete_command: false,
                },
            ));
        }
    };
    match cmd {
        Cmd::Model => {
            for m in src.models.iter().filter(|m| !m.hidden) {
                let cur = if m.id == src.current_model {
                    "  (current)"
                } else {
                    ""
                };
                add(
                    m.id.clone(),
                    format!("{}{cur}", m.display_name),
                    m.id.clone(),
                    &m.id,
                );
            }
        }
        Cmd::Effort => {
            for e in src.efforts {
                add(
                    e.as_str().to_string(),
                    String::new(),
                    e.as_str().to_string(),
                    e.as_str(),
                );
            }
        }
        Cmd::Permissions => {
            for a in Autonomy::ALL {
                add(
                    a.as_str().to_string(),
                    a.describe().to_string(),
                    a.as_str().to_string(),
                    a.as_str(),
                );
            }
            add("auto".into(), "same as low".into(), "auto".into(), "auto");
            add(
                "full-access".into(),
                "same as high".into(),
                "full-access".into(),
                "full-access",
            );
        }
        Cmd::Skills => {
            for sub in ["list", "show", "create", "path"] {
                add(sub.to_string(), String::new(), format!("{sub} "), sub);
            }
            if query.starts_with("show ") {
                out.clear();
                let q = query.trim_start_matches("show ").trim();
                for s in src.skills {
                    if let Some(m) = rank_label(q, &s.name) {
                        out.push((
                            m.score,
                            Item {
                                label: s.name.clone(),
                                detail: s.description.clone(),
                                insert: format!("show {}", s.name),
                                positions: m.positions,
                                kind: Kind::Skill,
                                complete_command: false,
                            },
                        ));
                    }
                }
            }
        }
        _ => {}
    }
    if !query.is_empty() {
        out.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    }
    out.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests;
