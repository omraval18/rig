//! Plain formatting helpers: durations, token counts, hidden messages and error explanations.

use rig_core::agent::AgentError;
use rig_core::message::{Block, Message, Role};
use std::time::Duration;

/// Environment reminders and compaction checkpoints are context, not conversation.
pub fn is_hidden(m: &Message) -> bool {
    let t = m.text();
    m.role == Role::User && rig_core::prompt::is_context(&t)
}

/// The text the user or model actually wrote, without injected `<rig-context>` context.
pub fn visible_text(m: &Message) -> String {
    m.blocks
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } if !rig_core::prompt::is_context(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// A failed turn in plain words, and whether sending the same prompt again could help.
pub(super) fn explain(e: &AgentError, provider: &str) -> (String, bool) {
    use rig_core::provider::ProviderError as P;
    let AgentError::Provider(p) = e else {
        return (e.to_string(), false);
    };
    match p {
        P::Network(m) => (
            format!(
                "Can't reach {provider}. Check your connection. ({})",
                m.trim_start_matches("io: ")
            ),
            true,
        ),
        P::Auth(m) if m.contains("/login") => (m.clone(), false),
        P::Auth(m) => (format!("Sign-in problem: {m}. Try /login."), false),
        P::Http { status: 429, .. } => (
            format!("{provider} is rate limiting requests. Wait a moment and retry."),
            true,
        ),
        P::Http { status, .. } if *status >= 500 => (
            format!("{provider} had a server error (HTTP {status}). Retrying usually works."),
            true,
        ),
        P::Protocol(m) if m.to_lowercase().contains("overloaded") => (
            format!("{provider} is overloaded right now. Retrying usually works."),
            true,
        ),
        other => (other.to_string(), false),
    }
}

/// A tool's wire name for people: `run_command` -> `Run command`.
pub fn tool_label(name: &str) -> String {
    let spaced = name.replace('_', " ");
    let mut chars = spaced.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// `45s`, `2m 05s`.
pub fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s}s")
    } else {
        format!("{}m {:02}s", s / 60, s % 60)
    }
}

pub fn fmt_tokens(n: u64) -> String {
    let trim = |v: f64, unit: &str| {
        let t = format!("{v:.1}");
        format!("{}{unit}", t.strip_suffix(".0").unwrap_or(&t))
    };
    if n >= 1_000_000 {
        trim(n as f64 / 1e6, "M")
    } else if n >= 1000 {
        trim(n as f64 / 1e3, "k")
    } else {
        n.to_string()
    }
}
