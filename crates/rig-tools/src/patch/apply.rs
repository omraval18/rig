//! Applying hunks to file contents in memory.

use super::*;

pub(super) fn apply_hunks(orig: &str, hunks: &[Hunk]) -> Result<String, String> {
    let had_trailing_newline = orig.ends_with('\n');
    let mut lines: Vec<String> = orig.split('\n').map(str::to_string).collect();
    if had_trailing_newline {
        lines.pop();
    }
    let mut cursor = 0usize;
    for h in hunks {
        if let Some(anchor) = &h.anchor {
            match (cursor..lines.len()).find(|&i| lines[i].trim() == anchor.trim()) {
                Some(i) => cursor = i + 1,
                None => return Err(format!("anchor line not found: {anchor:?}")),
            }
        }
        let old: Vec<&str> = h
            .lines
            .iter()
            .filter(|(t, _)| *t != '+')
            .map(|(_, s)| s.as_str())
            .collect();
        let new: Vec<String> = h
            .lines
            .iter()
            .filter(|(t, _)| *t != '-')
            .map(|(_, s)| s.clone())
            .collect();
        if old.is_empty() {
            let at = lines.len();
            lines.splice(at..at, new.iter().cloned());
            cursor = lines.len();
            continue;
        }
        let pos = seek(&lines, &old, cursor, h.at_eof)
            .or_else(|| seek(&lines, &old, 0, h.at_eof))
            .ok_or_else(|| format!("could not find the lines to change:\n{}", old.join("\n")))?;
        lines.splice(pos..pos + old.len(), new.iter().cloned());
        cursor = pos + new.len();
    }
    let mut out = lines.join("\n");
    if had_trailing_newline || orig.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

/// Find `needle` in `hay` at or after `from`, tolerating trailing-whitespace and then
/// indentation differences. With `at_eof`, prefer the match nearest the end.
pub(super) fn seek(hay: &[String], needle: &[&str], from: usize, at_eof: bool) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    let last = hay.len() - needle.len();
    let norms: [fn(&str) -> &str; 3] = [|s| s, |s| s.trim_end(), |s| s.trim()];
    for norm in norms {
        let matches = |start: usize| {
            needle
                .iter()
                .enumerate()
                .all(|(k, n)| norm(&hay[start + k]) == norm(n))
        };
        if at_eof && matches(last) {
            return Some(last);
        }
        if let Some(p) = (from..=last).find(|&s| matches(s)) {
            return Some(p);
        }
    }
    None
}
