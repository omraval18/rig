//! Turns `@path` and `$skill` tokens in a prompt into context for the model.
//!
//! The user's text is sent unchanged so the model sees what they wrote. What
//! these tokens reference is appended as `<rig-context>` blocks: small text
//! files are inlined, large ones are named so the model reads them on demand,
//! folders are listed, and skills contribute their instructions.

use crate::skills::{self, Skill};
use std::path::{Path, PathBuf};

const MAX_INLINE_FILE: usize = 24 * 1024;
const MAX_INLINE_TOTAL: usize = 80 * 1024;
const MAX_DIR_ENTRIES: usize = 100;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Kind {
    At,
    Dollar,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Token {
    pub kind: Kind,
    /// Char index of the sigil.
    pub start: usize,
    /// Char index one past the token.
    pub end: usize,
    /// Path or skill name, without the sigil or quotes.
    pub value: String,
}

fn starts_token(prev: Option<char>) -> bool {
    prev.is_none_or(|p| p.is_whitespace() || matches!(p, '(' | '[' | '{' | '<' | '\'' | '"' | '`'))
}

/// Find `@path` / `$skill` tokens. Quoted paths (`@"my file.txt"`) may contain spaces.
pub fn find_tokens(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (c == '@' || c == '$') && starts_token(i.checked_sub(1).map(|j| chars[j])) {
            let kind = if c == '@' { Kind::At } else { Kind::Dollar };
            let mut j = i + 1;
            let value: String;
            if kind == Kind::At && chars.get(j) == Some(&'"') {
                j += 1;
                let s = j;
                while j < chars.len() && chars[j] != '"' {
                    j += 1;
                }
                value = chars[s..j].iter().collect();
                if j < chars.len() {
                    j += 1;
                }
            } else {
                let s = j;
                while j < chars.len() && !chars[j].is_whitespace() {
                    if kind == Kind::Dollar
                        && !(chars[j].is_alphanumeric()
                            || matches!(chars[j], '-' | '_' | ':' | '.'))
                    {
                        break;
                    }
                    j += 1;
                }
                // Sentence punctuation after a mention is not part of it.
                while j > s
                    && matches!(
                        chars[j - 1],
                        ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '\'' | '`' | '.'
                    )
                    && !(chars[j - 1] == '.' && j - 1 > s && chars[j - 2] == '.')
                {
                    j -= 1;
                }
                value = chars[s..j].iter().collect();
            }
            if !value.is_empty() {
                out.push(Token {
                    kind,
                    start: i,
                    end: j,
                    value,
                });
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

pub fn resolve_path(value: &str, cwd: &Path, home: Option<&Path>) -> PathBuf {
    if value == "~" {
        return home
            .map(Path::to_path_buf)
            .unwrap_or_else(|| cwd.to_path_buf());
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return home.map_or_else(|| cwd.join(rest), |h| h.join(rest));
    }
    let p = Path::new(value);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

#[derive(Debug, Default)]
pub struct Expansion {
    /// Extra `<rig-context>` blocks to send alongside the user's text.
    pub reminders: Vec<String>,
    pub files: Vec<String>,
    pub dirs: Vec<String>,
    pub skills: Vec<String>,
    /// Pictures mentioned with `@path`, ready to send as image blocks.
    pub images: Vec<crate::message::Block>,
    /// Mentions that did not resolve (shown to the user, not sent).
    pub missing: Vec<String>,
}

impl Expansion {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
            && self.dirs.is_empty()
            && self.skills.is_empty()
            && self.images.is_empty()
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        for (n, noun) in [
            (self.files.len(), "file"),
            (self.images.len(), "image"),
            (self.dirs.len(), "folder"),
        ] {
            if n > 0 {
                parts.push(crate::text::count(n, noun));
            }
        }
        for s in &self.skills {
            parts.push(format!("skill {s}"));
        }
        parts.join(", ")
    }
}

pub fn expand(text: &str, cwd: &Path, home: Option<&Path>, skill_list: &[Skill]) -> Expansion {
    let mut ex = Expansion::default();
    let mut inlined = 0usize;
    let mut file_blocks = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for t in find_tokens(text) {
        if !seen.insert((t.kind == Kind::At, t.value.clone())) {
            continue;
        }
        match t.kind {
            // An unknown `$5` or `$HOME` in ordinary prose is not an error.
            Kind::Dollar => {
                if let Some(s) = skill_list.iter().find(|s| s.name == t.value) {
                    match skills::activation_text(s) {
                        Ok(txt) => {
                            ex.reminders.push(txt);
                            ex.skills.push(s.name.clone());
                        }
                        Err(_) => ex.missing.push(format!("${}", t.value)),
                    }
                }
            }
            Kind::At => {
                let path = resolve_path(&t.value, cwd, home);
                if path.is_dir() {
                    file_blocks.push(dir_block(&path));
                    ex.dirs.push(path.display().to_string());
                } else if path.is_file() && crate::image::mime_for_path(&path).is_some() {
                    match crate::image::load(&path) {
                        Ok(block) => ex.images.push(block),
                        Err(e) => ex.missing.push(format!("@{} ({e})", t.value)),
                    }
                } else if path.is_file() {
                    file_blocks.push(file_block(&path, &mut inlined));
                    ex.files.push(path.display().to_string());
                } else {
                    ex.missing.push(format!("@{}", t.value));
                }
            }
        }
    }
    if !file_blocks.is_empty() {
        ex.reminders.insert(0, format!("<rig-context>\nThe user mentioned these paths with @. Treat them as context for the request.\n\n{}\n</rig-context>", file_blocks.join("\n\n")));
    }
    ex
}

fn dir_block(path: &Path) -> String {
    let names = crate::fsutil::list_dir(path).unwrap_or_default();
    let total = names.len();
    let mut s = format!("## {} (folder, {total} entries)\n", path.display());
    for (n, d) in names.iter().take(MAX_DIR_ENTRIES) {
        s.push_str(&format!("{n}{}\n", if *d { "/" } else { "" }));
    }
    if total > MAX_DIR_ENTRIES {
        s.push_str(&format!("[{} more not shown]\n", total - MAX_DIR_ENTRIES));
    }
    s.trim_end().to_string()
}

fn file_block(path: &Path, inlined: &mut usize) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return format!("## {} (unreadable)", path.display());
    };
    if crate::text::looks_binary(&bytes) {
        return format!(
            "## {} (binary file, {} bytes; not inlined)",
            path.display(),
            bytes.len()
        );
    }
    if bytes.len() > MAX_INLINE_FILE || *inlined + bytes.len() > MAX_INLINE_TOTAL {
        return format!(
            "## {} (file, {} KB; not inlined, read it with the Read tool)",
            path.display(),
            bytes.len().div_ceil(1024)
        );
    }
    *inlined += bytes.len();
    let text = String::from_utf8_lossy(&bytes);
    format!(
        "## {} ({} lines)\n```\n{}\n```",
        path.display(),
        text.lines().count(),
        text.trim_end()
    )
}

#[cfg(test)]
mod tests;
