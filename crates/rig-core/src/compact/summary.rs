//! `/summary`: a short account of the session as it stands, for the user. Nothing in the history
//! changes; the result is shown, not saved.

use super::notes::clip;
use crate::message::{Message, Role};
use crate::prompt::is_context;
use crate::provider::{Effort, Provider, ProviderError, Request};
use std::sync::atomic::AtomicBool;

/// Most transcript text sent for one summary. The newest messages are kept when it is cut.
const MAX_INPUT: usize = 60_000;
const MAX_MESSAGE: usize = 2_000;

const SUMMARY_PROMPT: &str = "\
You summarize a coding session for the person who ran it. You are given the conversation as a \
transcript of their messages and the assistant's replies. Write a short Markdown summary with these \
sections, omitting any that would be empty:
## Goal
## Done
## Files changed
## Open items

Keep it under 200 words. Use only what the transcript says; do not invent files, commands or results.";

/// Summarize the conversation in `history`. Returns the summary text.
pub fn summarize(
    provider: &dyn Provider,
    history: &[Message],
    session_id: &str,
    cancel: &AtomicBool,
) -> Result<String, ProviderError> {
    let transcript = transcript(history);
    let req = Request {
        system: SUMMARY_PROMPT,
        messages: &[Message::user(format!(
            "Transcript:\n\n{transcript}\n\nWrite the summary now."
        ))],
        tools: &[],
        effort: Some(Effort::Low),
        session_id,
        cancel,
    };
    Ok(provider
        .stream(&req, &mut |_| {})?
        .message
        .text()
        .trim()
        .to_string())
}

/// The user's and the assistant's words, oldest first, keeping the newest within the budget.
fn transcript(history: &[Message]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0;
    for m in history.iter().rev() {
        let text = m.text();
        if text.trim().is_empty() || (m.role == Role::User && is_context(&text)) {
            continue;
        }
        let who = if m.role == Role::User {
            "User"
        } else {
            "Assistant"
        };
        let line = format!("{who}: {}", clip(text.trim(), MAX_MESSAGE));
        if used + line.len() > MAX_INPUT {
            break;
        }
        used += line.len();
        lines.push(line);
    }
    lines.reverse();
    lines.join("\n\n")
}
