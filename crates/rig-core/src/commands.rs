//! Slash-command registry. Pure data plus lookup and completion; the CLI owns the behavior.

use crate::fuzzy::{Match, rank_label};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    Help,
    New,
    Resume,
    Rename,
    Fork,
    Login,
    Logout,
    Provider,
    Profile,
    Stats,
    Status,
    Model,
    Effort,
    Permissions,
    Allowlist,
    Undo,
    Skills,
    Copy,
    Paste,
    Compact,
    Summary,
    Loop,
    Reasoning,
    Settings,
    Version,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    General,
    Session,
    Model,
    Account,
    Extensions,
}

impl Category {
    pub fn title(self) -> &'static str {
        match self {
            Category::General => "General",
            Category::Session => "Session",
            Category::Model => "Model",
            Category::Account => "Account",
            Category::Extensions => "Extensions",
        }
    }
}

#[derive(Debug)]
pub struct Spec {
    pub cmd: Cmd,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub summary: &'static str,
    pub category: Category,
    /// Takes an argument, so completing it appends a space instead of running it.
    pub has_args: bool,
}

const fn spec(
    cmd: Cmd,
    name: &'static str,
    usage: &'static str,
    summary: &'static str,
    category: Category,
    has_args: bool,
) -> Spec {
    Spec {
        cmd,
        name,
        aliases: &[],
        usage,
        summary,
        category,
        has_args,
    }
}

pub static COMMANDS: &[Spec] = &[
    spec(
        Cmd::Help,
        "help",
        "/help",
        "show available slash commands",
        Category::General,
        false,
    ),
    Spec {
        aliases: &["clear"],
        ..spec(
            Cmd::New,
            "new",
            "/new (/clear)",
            "start a fresh session",
            Category::Session,
            false,
        )
    },
    spec(
        Cmd::Resume,
        "resume",
        "/resume",
        "resume a saved session",
        Category::Session,
        false,
    ),
    spec(
        Cmd::Rename,
        "rename",
        "/rename <title>",
        "rename the current session",
        Category::Session,
        true,
    ),
    spec(
        Cmd::Fork,
        "fork",
        "/fork",
        "fork the session before an earlier prompt",
        Category::Session,
        false,
    ),
    spec(
        Cmd::Compact,
        "compact",
        "/compact",
        "summarize older context into a fresh window",
        Category::Session,
        false,
    ),
    spec(
        Cmd::Undo,
        "undo",
        "/undo",
        "undo the latest tracked file edit",
        Category::Session,
        false,
    ),
    spec(
        Cmd::Paste,
        "paste",
        "/paste",
        "attach the image on your clipboard (or press Ctrl+V)",
        Category::Session,
        false,
    ),
    spec(
        Cmd::Copy,
        "copy",
        "/copy",
        "copy the last assistant response",
        Category::Session,
        false,
    ),
    spec(
        Cmd::Model,
        "model",
        "/model [id-or-query]",
        "choose the model",
        Category::Model,
        true,
    ),
    spec(
        Cmd::Effort,
        "effort",
        "/effort [level]",
        "choose the reasoning effort",
        Category::Model,
        true,
    ),
    Spec {
        aliases: &["goal"],
        ..spec(
            Cmd::Loop,
            "loop",
            "/loop <objective> (/goal)",
            "keep working toward an objective across turns until it is done",
            Category::Session,
            true,
        )
    },
    spec(
        Cmd::Summary,
        "summary",
        "/summary",
        "summarize this session (the history is not changed)",
        Category::Session,
        false,
    ),
    spec(
        Cmd::Reasoning,
        "reasoning",
        "/reasoning",
        "show or hide the model's reasoning summary",
        Category::Model,
        false,
    ),
    spec(
        Cmd::Permissions,
        "permissions",
        "/permissions [level]",
        "choose what rig may do without asking",
        Category::Model,
        true,
    ),
    spec(
        Cmd::Allowlist,
        "allowlist",
        "/allowlist [view|remove <n>|reset]",
        "manage commands you chose to always allow",
        Category::Model,
        true,
    ),
    Spec {
        aliases: &["setup"],
        ..spec(
            Cmd::Provider,
            "provider",
            "/provider (/setup)",
            "choose the model provider",
            Category::Account,
            false,
        )
    },
    spec(
        Cmd::Profile,
        "profile",
        "/profile [name]",
        "switch to another profile (its own logins, keys and settings)",
        Category::Account,
        true,
    ),
    spec(
        Cmd::Login,
        "login",
        "/login",
        "sign in with your ChatGPT subscription",
        Category::Account,
        false,
    ),
    spec(
        Cmd::Logout,
        "logout",
        "/logout",
        "sign out of ChatGPT",
        Category::Account,
        false,
    ),
    spec(
        Cmd::Stats,
        "stats",
        "/stats",
        "show token and turn statistics",
        Category::Account,
        false,
    ),
    spec(
        Cmd::Status,
        "status",
        "/status",
        "show configuration and the session id",
        Category::General,
        false,
    ),
    spec(
        Cmd::Skills,
        "skills",
        "/skills [list|show|create|path]",
        "browse skills ($ opens skill search)",
        Category::Extensions,
        true,
    ),
    spec(
        Cmd::Settings,
        "settings",
        "/settings [files]",
        "change model, effort, permissions and other defaults",
        Category::General,
        true,
    ),
    spec(
        Cmd::Version,
        "version",
        "/version",
        "show the rig version",
        Category::General,
        false,
    ),
    Spec {
        aliases: &["exit", "q"],
        ..spec(
            Cmd::Quit,
            "quit",
            "/quit (/exit)",
            "exit rig",
            Category::General,
            false,
        )
    },
];

pub fn lookup(token: &str) -> Option<&'static Spec> {
    let t = token.trim_start_matches('/').to_lowercase();
    COMMANDS
        .iter()
        .find(|s| s.name == t || s.aliases.contains(&t.as_str()))
}

pub struct Completion {
    pub spec: &'static Spec,
    pub m: Match,
}

/// Commands matching `query` (the text after `/`), best first. Empty query lists everything in registry order.
pub fn complete(query: &str) -> Vec<Completion> {
    let mut out: Vec<Completion> = COMMANDS
        .iter()
        .filter_map(|s| {
            let best = std::iter::once(s.name)
                .chain(s.aliases.iter().copied())
                .filter_map(|n| rank_label(query, n))
                .max_by_key(|m| m.score)?;
            Some(Completion { spec: s, m: best })
        })
        .collect();
    if !query.is_empty() {
        out.sort_by_key(|c| std::cmp::Reverse(c.m.score));
    }
    out
}

/// Plain-text help, grouped by category.
pub fn help_text() -> String {
    let mut out = String::new();
    let width = COMMANDS
        .iter()
        .map(|s| s.usage.chars().count())
        .max()
        .unwrap_or(0);
    for cat in [
        Category::General,
        Category::Session,
        Category::Model,
        Category::Account,
        Category::Extensions,
    ] {
        out.push_str(&format!("{}\n", cat.title()));
        for s in COMMANDS.iter().filter(|s| s.category == cat) {
            out.push_str(&format!("  {:<width$}  {}\n", s.usage, s.summary));
        }
    }
    out.push_str("\nType @ to mention a file or folder, $ to use a skill, / for commands.\n");
    out
}

#[cfg(test)]
mod tests;
