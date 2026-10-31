//! Markdown for streamed assistant text, one line at a time.
//!
//! Replies arrive as whole lines, so this is a line renderer with one bit of state (inside a code
//! fence) and no parser: markers are hidden and replaced by styles, and anything that does not
//! close on its own line is left as typed. It is a single pass over the bytes with no allocation
//! for text that has nothing to style.

use crate::style::{BLUE, BOLD, CYAN, DIM, ITALIC, RESET, STRIKE, UNDERLINE};

const H1: &str = "\x1b[1;4m";
/// Width of a horizontal rule; replies are wrapped to the window afterwards.
const RULE: usize = 40;
/// Bytes that can start inline markup.
const SPECIAL: &[u8] = b"`*_~[\\";
/// Nested emphasis stops here: `***a***` is as deep as prose goes.
const MAX_DEPTH: usize = 4;

#[derive(Default)]
pub struct Styler {
    in_code: bool,
    pub color: bool,
}

impl Styler {
    pub fn new(color: bool) -> Self {
        Self {
            in_code: false,
            color,
        }
    }

    pub fn line(&mut self, line: &str) -> String {
        if !self.color {
            return line.to_string();
        }
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            self.in_code = !self.in_code;
            return format!("{DIM}{line}{RESET}");
        }
        if self.in_code {
            return format!("{CYAN}{line}{RESET}");
        }
        if let Some((level, text)) = heading(trimmed) {
            let style = if level == 1 { H1 } else { BOLD };
            return format!("{style}{}{RESET}", inline(text, style));
        }
        if let Some(rest) = trimmed.strip_prefix('>') {
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            return format!("{indent}{DIM}│ {}{RESET}", inline(rest, DIM));
        }
        if is_rule(trimmed) {
            return format!("{DIM}{}{RESET}", "─".repeat(RULE));
        }
        if let Some(rest) = bullet(trimmed) {
            let mark = if indent.len() >= 2 { '◦' } else { '•' };
            return format!("{indent}{DIM}{mark}{RESET} {}", inline(&task(rest), ""));
        }
        if trimmed.starts_with('|') {
            return table_row(trimmed);
        }
        inline(line, "")
    }
}

/// `## Title` -> (2, "Title"). A `#` needs a space after it: `#tag` is not a heading.
fn heading(t: &str) -> Option<(usize, &str)> {
    let level = t.bytes().take_while(|b| *b == b'#').count();
    let rest = &t[level..];
    if !(1..=6).contains(&level) || !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    Some((level, rest.trim().trim_end_matches('#').trim_end()))
}

fn is_rule(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() >= 3 && matches!(b[0], b'-' | b'*' | b'_') && b.iter().all(|c| *c == b[0])
}

/// The text after a `-`, `*` or `+` list marker.
fn bullet(t: &str) -> Option<&str> {
    ["- ", "* ", "+ "].iter().find_map(|m| t.strip_prefix(m))
}

/// `[ ] todo` and `[x] done` become checkboxes.
fn task(rest: &str) -> String {
    for (open, box_) in [("[ ] ", "☐ "), ("[x] ", "☑ "), ("[X] ", "☑ ")] {
        if let Some(r) = rest.strip_prefix(open) {
            return format!("{box_}{r}");
        }
    }
    rest.to_string()
}

/// Cells keep the author's spacing (a stream cannot align columns it has not seen yet); the
/// pipes dim and the `|---|` row becomes a rule.
fn table_row(t: &str) -> String {
    let pipe = format!("{DIM}│{RESET}");
    if t.contains('-') && t.bytes().all(|b| matches!(b, b'|' | b'-' | b':' | b' ')) {
        let rule: String = t
            .chars()
            .map(|c| {
                if c == '|' {
                    '│'
                } else if c == ' ' {
                    ' '
                } else {
                    '─'
                }
            })
            .collect();
        return format!("{DIM}{rule}{RESET}");
    }
    t.split('|')
        .map(|cell| inline(cell, ""))
        .collect::<Vec<_>>()
        .join(&pipe)
}

/// Inline markup in `s`. `base` is the style the line already has (a quote's dim, a heading's
/// bold): it is restored after every span so a span never cancels it.
fn inline(s: &str, base: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    emit(&mut out, s, base, 0);
    out
}

fn emit(out: &mut String, s: &str, base: &str, depth: usize) {
    let b = s.as_bytes();
    let (mut i, mut done) = (0, 0);
    while i < b.len() {
        if !SPECIAL.contains(&b[i]) {
            i += 1;
            continue;
        }
        let run = b[i..].iter().take_while(|c| **c == b[i]).count();
        let span = match b[i] {
            b'\\' => {
                // `\*` is a literal star: drop the backslash and skip what it protects.
                if b.get(i + 1).is_some_and(u8::is_ascii_punctuation) {
                    out.push_str(&s[done..i]);
                    done = i + 1;
                    i += 2;
                    continue;
                }
                None
            }
            b'`' => code_span(s, i, run).map(|(a, z, end)| Span::Code(&s[a..z], end)),
            b'[' => link(s, i).map(|(text, url, end)| Span::Link(text, url, end)),
            b'~' if run >= 2 && depth < MAX_DEPTH => {
                closer(s, i, 2).map(|(a, z, end)| Span::Mark(STRIKE, &s[a..z], end))
            }
            b'*' | b'_' if depth < MAX_DEPTH => {
                let width = run.min(2);
                closer(s, i, width).map(|(a, z, end)| {
                    Span::Mark(if width == 2 { BOLD } else { ITALIC }, &s[a..z], end)
                })
            }
            _ => None,
        };
        let Some(span) = span else {
            // An unmatched marker stays as typed, along with the rest of its run.
            i += run;
            continue;
        };
        out.push_str(&s[done..i]);
        i = span.write(out, base, depth);
        done = i;
    }
    out.push_str(&s[done..]);
}

enum Span<'a> {
    Code(&'a str, usize),
    Link(&'a str, &'a str, usize),
    Mark(&'static str, &'a str, usize),
}

impl Span<'_> {
    /// Write the span and return where the text after it starts.
    fn write(&self, out: &mut String, base: &str, depth: usize) -> usize {
        match *self {
            Span::Code(text, end) => {
                out.push_str(CYAN);
                out.push_str(text);
                close(out, base);
                end
            }
            Span::Mark(style, inner, end) => {
                out.push_str(style);
                emit(out, inner, &format!("{base}{style}"), depth + 1);
                close(out, base);
                end
            }
            Span::Link(text, url, end) => {
                let style = format!("{UNDERLINE}{BLUE}");
                out.push_str(&style);
                emit(out, text, &format!("{base}{style}"), depth + 1);
                close(out, base);
                if url != text {
                    out.push_str(&format!(" {DIM}({url})"));
                    close(out, base);
                }
                end
            }
        }
    }
}

fn close(out: &mut String, base: &str) {
    out.push_str(RESET);
    out.push_str(base);
}

/// The backtick run at `i` (`n` long) and the next run of exactly `n`: (start, end, after).
fn code_span(s: &str, i: usize, n: usize) -> Option<(usize, usize, usize)> {
    let b = s.as_bytes();
    let mut j = i + n;
    while let Some(p) = s[j..].find('`') {
        let at = j + p;
        let len = b[at..].iter().take_while(|c| **c == b'`').count();
        if len == n {
            return Some((i + n, at, at + n));
        }
        j = at + len;
    }
    None
}

/// `[text](url)` at `i`: (text, url, after). A title after the url is dropped.
fn link(s: &str, i: usize) -> Option<(&str, &str, usize)> {
    let close = i + s[i..].find("](")?;
    let text = &s[i + 1..close];
    let end = close + 2 + s[close + 2..].find(')')?;
    let url = s[close + 2..end].split_whitespace().next()?;
    (!text.is_empty()).then_some((text, url, end + 1))
}

/// Emphasis opened by `width` marker bytes at `i`: (inner start, inner end, after the closer).
/// The opener must touch text, and so must the closer; `_` also may not sit inside a word, so
/// `snake_case` and `2 * 3 * 4` are left alone.
fn closer(s: &str, i: usize, width: usize) -> Option<(usize, usize, usize)> {
    let b = s.as_bytes();
    let mark = b[i];
    let start = i + width;
    if b.get(start).is_none_or(u8::is_ascii_whitespace) {
        return None;
    }
    let word = |c: char| c.is_alphanumeric();
    if mark == b'_' && s[..i].chars().next_back().is_some_and(word) {
        return None;
    }
    let mut j = start;
    while let Some(p) = s[j..].find(mark as char) {
        let at = j + p;
        let len = b[at..].iter().take_while(|c| **c == mark).count();
        j = at + len;
        // A longer run belongs to emphasis nested inside, except that it may end ours.
        let usable = if width == 1 { len == 1 } else { len >= 2 };
        if !usable || at == start || b[at - 1].is_ascii_whitespace() {
            continue;
        }
        let end = if width == 2 { j } else { at + 1 };
        let at = if width == 2 { j - 2 } else { at };
        if mark == b'_' && s[end..].chars().next().is_some_and(word) {
            continue;
        }
        return Some((start, at, end));
    }
    None
}

#[cfg(test)]
mod tests;
