//! Building the summarizer input and checking its notes against the archive.

use super::*;
use std::collections::HashSet;

pub(super) fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max * 2 / 3).collect();
    let tail: String = s.chars().skip(s.chars().count() - max / 3).collect();
    format!("{head} [...] {tail}")
}

/// Plain-text transcript of the head for the summarizer, with handles.
pub(super) fn transcript(recs: &[Record], scale: usize) -> String {
    let mut s = String::new();
    for r in recs {
        let v = &r.json;
        if r.id.starts_with('M') {
            let cap = if v["role"] == "user" { 4000 } else { 2000 } / scale;
            s.push_str(&format!(
                "[{}] {}: {}\n\n",
                r.id,
                v["role"].as_str().unwrap_or(""),
                clip(v["text"].as_str().unwrap_or(""), cap)
            ));
        } else {
            s.push_str(&format!(
                "[{}] tool {} {}\n  -> {}{}\n\n",
                r.id,
                v["name"].as_str().unwrap_or(""),
                clip(&v["args"].to_string(), 300 / scale),
                if v["is_error"] == true {
                    "(error) "
                } else {
                    ""
                },
                clip(v["output"].as_str().unwrap_or(""), 700 / scale)
            ));
        }
    }
    s
}

/// Replace handles the archive does not contain with "(unverified)".
pub fn lint(notes: &str, known: &HashSet<String>) -> (String, usize) {
    let mut out = String::with_capacity(notes.len());
    let mut bad = 0;
    let chars: Vec<char> = notes.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if (chars[i] == 'M' || chars[i] == 'T')
            && chars.get(i + 1).is_some_and(char::is_ascii_digit)
            && i.checked_sub(1).is_none_or(|p| !chars[p].is_alphanumeric())
        {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            let id: String = chars[i..j].iter().collect();
            if known.contains(&id) {
                out.push_str(&id);
            } else {
                out.push_str("(unverified)");
                bad += 1;
            }
            i = j;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    (out, bad)
}

/// The body of an earlier checkpoint without its wrapper or intro paragraph, so it can be carried forward.
pub(super) fn strip_checkpoint(text: &str) -> String {
    let t = text
        .replace("<rig-context>", "")
        .replace("</rig-context>", "")
        .replace("<system-reminder>", "")
        .replace("</system-reminder>", "");
    let t = t.trim();
    let after_intro = t.find("\n\n").map_or(t, |i| &t[i + 2..]);
    let (kept, notes) = after_intro
        .split_once("\n## Notes\n")
        .map_or((after_intro, ""), |(a, b)| (a, b));
    let mut out = kept.trim().to_string();
    if !notes.trim().is_empty() {
        out.push_str(&format!(
            "\n\n## Earlier notes\n{}",
            clip(notes.trim(), 8_000)
        ));
    }
    out
}

pub(super) fn exact_sections(recs: &[Record], prior_checkpoints: &[String]) -> String {
    let mut users = Vec::new();
    let mut replies = Vec::new();
    for r in recs.iter().filter(|r| r.id.starts_with('M')) {
        let text = r.json["text"].as_str().unwrap_or("");
        if crate::prompt::is_context(text) {
            continue;
        }
        if r.json["role"] == "user" {
            users.push(format!("[{}] {}", r.id, clip(text, MAX_USER_TEXT)));
        } else {
            replies.push(format!("[{}] {}", r.id, clip(text, MAX_REPLY_TEXT)));
        }
    }
    let mut s = String::new();
    for p in prior_checkpoints {
        s.push_str(&clip(p.trim(), MAX_EXACT_SECTION));
        s.push_str("\n\n");
    }
    let mut section = |title: &str, items: Vec<String>| {
        if items.is_empty() {
            return;
        }
        let mut body = String::new();
        // Drop the oldest first when over budget.
        for item in items.iter().rev() {
            if body.len() + item.len() > MAX_EXACT_SECTION / 2 {
                break;
            }
            body = format!("{item}\n\n{body}");
        }
        s.push_str(&format!("## {title}\n{}\n", body.trim_end()));
    };
    section("User messages in the compacted part (exact)", users);
    section("Assistant replies in the compacted part (exact)", replies);
    s
}
