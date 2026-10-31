//! Context compaction.
//!
//! When the window fills, older turns are replaced by one checkpoint message and
//! the conversation continues in the same session. Nothing is lost:
//!
//! - the newest exchanges stay untouched (`window`);
//! - every user message and final reply in the compacted part is kept word for word;
//! - the model writes notes for the rest, and each note may cite handles
//!   (`M3` for a message, `T7` for a tool call) that `lint` checks against the archive;
//! - the full text of everything compacted is archived to disk so the agent can
//!   look it up later with the `recall` tool.

use crate::message::{Block, Message, Role};
use crate::provider::{Effort, Provider, ProviderError, Request};
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::AtomicBool;

mod archive;
mod notes;

pub use archive::{lookup, search};

use archive::{Record, known_ids, next_ids, records};
use notes::{clip, exact_sections, lint, strip_checkpoint, transcript};

const CHECKPOINT_MARK: &str = "Earlier part of this conversation was compacted";
const MAX_USER_TEXT: usize = 6_000;
const MAX_REPLY_TEXT: usize = 2_000;
const MAX_EXACT_SECTION: usize = 40_000;
const MAX_TRANSCRIPT: usize = 120_000;
const MIN_COMPACTABLE_TOKENS: usize = 1_500;

const SUMMARIZER_PROMPT: &str = "\
You write working notes so a coding agent can continue a long session after its older messages are removed. \
You are given the removed part as a transcript. Messages carry handles like [M3] and tool calls carry handles like [T7].

Write concise Markdown with these sections, omitting any that would be empty:
## Goal
## Decisions and findings
## Files and commands touched
## Current state
## Open work

Rules:
- Record facts, paths, commands, error messages and decisions the agent will need again. Skip chatter.
- Cite the handle of the message or tool call that supports a claim, like [T7] or [M3].
- Never invent a handle, a path, or a result that is not in the transcript.
- Do not repeat the user's messages; they are kept separately, word for word.";

pub fn is_checkpoint(m: &Message) -> bool {
    m.role == Role::User && m.text().contains(CHECKPOINT_MARK)
}

/// Rough token count: 4 bytes per token, plus a little per block.
pub fn estimate_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .flat_map(|m| &m.blocks)
        .map(|b| match b {
            Block::Text { text } => text.len() / 4 + 4,
            Block::Reasoning { text, raw } => {
                text.len() / 4 + raw.as_ref().map_or(0, |o| o.data.to_string().len() / 4)
            }
            Block::ToolCall { name, args, .. } => (name.len() + args.to_string().len()) / 4 + 8,
            Block::ToolResult { output, .. } => output.len() / 4 + 6,
            // Providers bill an image by its size, not its bytes; this is a typical screenshot.
            Block::Image { .. } => 1_500,
        })
        .sum()
}

fn is_turn_start(m: &Message) -> bool {
    m.role == Role::User
        && m.blocks.iter().any(|b| matches!(b, Block::Text { .. }))
        && !is_checkpoint(m)
}

/// Where the kept tail starts. The head `[1..split)` is compacted; index 0 (environment reminder) is always kept.
///
/// A split must not separate a tool call from its result, so it lands on a turn start or on an
/// assistant message. The tail keeps at least the last two turns, or, inside one long turn,
/// roughly the last `tail_budget_tokens` of exchanges.
pub fn split_point(history: &[Message], tail_budget_tokens: usize) -> Option<usize> {
    if history.len() < 5 {
        return None;
    }
    let starts: Vec<usize> = (1..history.len())
        .filter(|&i| is_turn_start(&history[i]))
        .collect();
    // Preferred: keep the last two turns whole.
    if starts.len() >= 3 {
        let s = starts[starts.len() - 2];
        if estimate_tokens(&history[s..]) <= tail_budget_tokens * 2 {
            return Some(s);
        }
    }
    // Otherwise walk back from the end accumulating the tail budget, then snap to a legal boundary.
    let mut acc = 0usize;
    let mut idx = history.len();
    while idx > 2 && acc < tail_budget_tokens {
        idx -= 1;
        acc += estimate_tokens(&history[idx..idx + 1]);
    }
    (idx..history.len())
        .find(|&j| j >= 2 && (is_turn_start(&history[j]) || history[j].role == Role::Assistant))
}

#[derive(Debug)]
pub struct Checkpoint {
    /// Compacted range is `1..split`; it becomes `message`.
    pub split: usize,
    pub message: Message,
    pub archived_messages: usize,
    pub archived_tool_calls: usize,
    pub unverified_handles: usize,
}

/// Compact `history` in place. `Ok(None)` means there was nothing worth compacting.
pub fn compact(
    provider: &dyn Provider,
    history: &mut Vec<Message>,
    archive: &Path,
    tail_budget_tokens: usize,
    session_id: &str,
    cancel: &AtomicBool,
) -> Result<Option<Checkpoint>, ProviderError> {
    let Some(split) = split_point(history, tail_budget_tokens) else {
        return Ok(None);
    };
    if split <= 1 || estimate_tokens(&history[1..split]) < MIN_COMPACTABLE_TOKENS {
        return Ok(None);
    }
    let head: Vec<Message> = history[1..split].to_vec();
    let prior: Vec<String> = head
        .iter()
        .filter(|m| is_checkpoint(m))
        .map(Message::text)
        .collect();
    let content: Vec<Message> = head.iter().filter(|m| !is_checkpoint(m)).cloned().collect();

    let (m0, t0) = next_ids(archive);
    let recs = records(&content, m0, t0);
    let known: HashSet<String> = recs
        .iter()
        .map(|r| r.id.clone())
        .chain(known_ids(archive))
        .collect();

    // Summarize first; only archive and mutate once that has succeeded.
    let mut scale = 1;
    let mut text = transcript(&recs, scale);
    while text.len() > MAX_TRANSCRIPT && scale < 8 {
        scale *= 2;
        text = transcript(&recs, scale);
    }
    let mut prompt = String::new();
    for p in &prior {
        prompt.push_str(&format!("Notes from an earlier compaction:\n{p}\n\n"));
    }
    prompt.push_str(&format!(
        "Transcript of the part being removed:\n\n{text}\nWrite the notes now."
    ));
    let req = Request {
        system: SUMMARIZER_PROMPT,
        messages: &[Message::user(prompt)],
        tools: &[],
        effort: Some(Effort::Low),
        session_id,
        cancel,
    };
    let notes = provider.stream(&req, &mut |_| {})?.message.text();
    let (notes, unverified) = lint(notes.trim(), &known);

    let tool_calls = recs.iter().filter(|r| r.id.starts_with('T')).count();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(archive)
    {
        for r in &recs {
            let _ = writeln!(f, "{}", r.json);
        }
    }
    let carried: Vec<String> = prior.iter().map(|p| strip_checkpoint(p)).collect();
    let body = format!(
        "<rig-context>\n{CHECKPOINT_MARK} to free context. The full text of every earlier message (M#) and tool call (T#) is archived. Use the recall tool with words or a handle such as T7 to read it back; do not guess details you no longer see.\n\n{}\n## Notes\n{}\n</rig-context>",
        exact_sections(&recs, &carried),
        notes
    );
    let message = Message::user(body);
    let archived_messages = recs.len() - tool_calls;
    history.splice(1..split, std::iter::once(message.clone()));
    Ok(Some(Checkpoint {
        split,
        message,
        archived_messages,
        archived_tool_calls: tool_calls,
        unverified_handles: unverified,
    }))
}

#[cfg(test)]
mod tests;
