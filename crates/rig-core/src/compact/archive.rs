//! The on-disk archive of compacted messages and tool calls, and looking things up in it.

use super::*;
use crate::message::{Block, Message, Role};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(super) struct Record {
    pub(super) id: String,
    pub(super) json: Value,
}

/// Archive records for `head`: M# for text messages, T# for tool calls with their results.
pub(super) fn records(head: &[Message], start_m: usize, start_t: usize) -> Vec<Record> {
    let mut results: HashMap<&str, (&str, bool)> = HashMap::new();
    for m in head {
        for b in &m.blocks {
            if let Block::ToolResult {
                id,
                output,
                is_error,
            } = b
            {
                results.insert(id.as_str(), (output.as_str(), *is_error));
            }
        }
    }
    let (mut mn, mut tn) = (start_m, start_t);
    let mut out = Vec::new();
    for m in head {
        let role = if m.role == Role::User {
            "user"
        } else {
            "assistant"
        };
        for b in &m.blocks {
            match b {
                Block::Text { text } if !text.trim().is_empty() => {
                    mn += 1;
                    let id = format!("M{mn}");
                    out.push(Record {
                        json: json!({"id": id, "role": role, "text": text}),
                        id,
                    });
                }
                Block::ToolCall {
                    id: call,
                    name,
                    args,
                } => {
                    tn += 1;
                    let id = format!("T{tn}");
                    let (output, is_error) =
                        results.get(call.as_str()).copied().unwrap_or(("", false));
                    out.push(Record { json: json!({"id": id, "name": name, "args": args, "output": output, "is_error": is_error}), id });
                }
                _ => {}
            }
        }
    }
    out
}

pub(super) fn next_ids(archive: &Path) -> (usize, usize) {
    let (mut m, mut t) = (0, 0);
    if let Ok(text) = std::fs::read_to_string(archive) {
        for line in text.lines() {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                let id = v["id"].as_str().unwrap_or("");
                let n: usize = id[1.min(id.len())..].parse().unwrap_or(0);
                match id.chars().next() {
                    Some('M') => m = m.max(n),
                    Some('T') => t = t.max(n),
                    _ => {}
                }
            }
        }
    }
    (m, t)
}

pub(super) fn known_ids(archive: &Path) -> HashSet<String> {
    std::fs::read_to_string(archive)
        .map(|t| {
            t.lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .filter_map(|v| v["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Read one archived record by id ("T7", "M12").
pub fn lookup(archive: &Path, id: &str) -> Option<String> {
    let text = std::fs::read_to_string(archive).ok()?;
    for line in text.lines() {
        let v: Value = serde_json::from_str(line).ok()?;
        if v["id"].as_str().is_some_and(|i| i.eq_ignore_ascii_case(id)) {
            return Some(render_record(&v, 8_000));
        }
    }
    None
}

fn render_record(v: &Value, cap: usize) -> String {
    let id = v["id"].as_str().unwrap_or("?");
    if id.starts_with('M') {
        format!(
            "[{id}] {}: {}",
            v["role"].as_str().unwrap_or(""),
            clip(v["text"].as_str().unwrap_or(""), cap)
        )
    } else {
        format!(
            "[{id}] tool {} {}\n{}",
            v["name"].as_str().unwrap_or(""),
            v["args"],
            clip(v["output"].as_str().unwrap_or(""), cap)
        )
    }
}

/// Case-insensitive search over the archive. Every phrase must appear (AND); at most `limit` hits.
pub fn search(archive: &Path, phrases: &[String], limit: usize) -> Vec<String> {
    let needles: Vec<String> = phrases
        .iter()
        .map(|p| p.to_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    let Ok(text) = std::fs::read_to_string(archive) else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let hay = match v["id"].as_str().unwrap_or("").chars().next() {
            Some('M') => v["text"].as_str().unwrap_or("").to_lowercase(),
            _ => format!(
                "{} {} {}",
                v["name"].as_str().unwrap_or(""),
                v["args"],
                v["output"].as_str().unwrap_or("")
            )
            .to_lowercase(),
        };
        if needles.iter().all(|n| hay.contains(n)) {
            hits.push(render_record(&v, 600));
            if hits.len() >= limit {
                break;
            }
        }
    }
    hits
}
