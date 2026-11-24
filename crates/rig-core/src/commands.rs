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
    Mcp,
    RefreshModels,
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
    pub const ALL: [Category; 5] = [
        Category::Model,
        Category::Account,
        Category::Session,
        Category::Extensions,
        Category::General,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::General => "General",
            Category::Session => "Session",
            Category::Model => "Model & permissions",
            Category::Account => "Providers & profiles",
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
        Cmd::Model,
        "model",
        "/model [query | provider/model | -]",
        "choose from every connected provider (Ctrl+P); - goes back",
        Category::Model,
        true,
    ),
    Spec {
        aliases: &["connect", "setup"],
        ..spec(
            Cmd::Provider,
            "provider",
            "/provider [name] (/connect)",
            "connect and manage providers: sign in, add a key, add an endpoint",
            Category::Account,
            true,
        )
    },
    spec(
        Cmd::Profile,
        "profile",
        "/profile [name | new <name> | remove <name>]",
        "switch, create or remove profiles (Ctrl+O)",
        Category::Account,
        true,
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
        Cmd::Effort,
        "effort",
        "/effort [level]",
        "choose the reasoning effort",
        Category::Model,
        true,
    ),
    spec(
        Cmd::Permissions,
        "permissions",
        "/permissions [level]",
        "choose what rig may do without asking",
        Category::Model,
        true,
    ),
    Spec {
        aliases: &["refresh-models"],
        ..spec(
            Cmd::RefreshModels,
            "refresh",
            "/refresh",
            "get the latest models from every connected provider, and prices",
            Category::Model,
            false,
        )
    },
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
        Cmd::Fork,
        "fork",
        "/fork",
        "fork the session before an earlier prompt",
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
        Cmd::Paste,
        "paste",
        "/paste",
        "attach the image on your clipboard (or press Ctrl+V)",
        Category::Session,
        false,
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
        Cmd::Rename,
        "rename",
        "/rename <title>",
        "rename the current session",
        Category::Session,
        true,
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
        Cmd::Mcp,
        "mcp",
        "/mcp",
        "show the MCP servers and whether they are running",
        Category::Extensions,
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
        Cmd::Stats,
        "stats",
        "/stats",
        "show token and turn statistics",
        Category::General,
        false,
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
        Cmd::Login,
        "login",
        "/login [provider]",
        "sign in to ChatGPT, or enter a key for another provider",
        Category::Account,
        true,
    ),
    spec(
        Cmd::Logout,
        "logout",
        "/logout [provider]",
        "sign out of ChatGPT, or forget a saved key",
        Category::Account,
        true,
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
        Cmd::Allowlist,
        "allowlist",
        "/allowlist [view|remove <n>|reset]",
        "manage commands you chose to always allow",
        Category::Model,
        true,
    ),
    spec(
        Cmd::Help,
        "help",
        "/help",
        "browse commands and keyboard shortcuts",
        Category::General,
        false,
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

/// The command someone probably meant by a mistyped `token`.
pub fn suggest(token: &str) -> Option<&'static Spec> {
    let t = token.trim_start_matches('/').to_lowercase();
    COMMANDS
        .iter()
        .flat_map(|s| {
            std::iter::once(s.name)
                .chain(s.aliases.iter().copied())
                .map(move |n| (s, n))
        })
        .map(|(s, n)| (distance(&t, n), s))
        .filter(|(d, _)| *d <= 2 && *d < t.chars().count())
        .min_by_key(|(d, _)| *d)
        .map(|(_, s)| s)
}

/// Edits (insert, delete, replace) between two short words.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let replace = diagonal + usize::from(ca != *cb);
            diagonal = row[j + 1];
            row[j + 1] = replace.min(diagonal + 1).min(row[j] + 1);
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests;
