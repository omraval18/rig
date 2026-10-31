//! Width-aware text helpers: truncate, wrap, highlight and style prompt rows.

use crate::style::{self, CYAN, DIM, GREEN, MAGENTA, RESET, YELLOW};
use rig_core::expand::{self, Kind as TokKind};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn width_of(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Cut `s` to at most `max` columns, adding `…` when something was dropped.
pub fn truncate(s: &str, max: usize) -> String {
    if width_of(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// `text` with the chars at `positions` highlighted, cut to `max` columns.
pub(super) fn highlighted(
    text: &str,
    positions: &[usize],
    base: &str,
    hl: &str,
    max: usize,
    color: bool,
) -> String {
    let shown = truncate(text, max);
    if !color {
        return shown;
    }
    let mut out = String::new();
    for (i, c) in shown.chars().enumerate() {
        if positions.contains(&i) {
            out.push_str(&format!("{RESET}{hl}{c}{RESET}{base}"));
        } else {
            out.push(c);
        }
    }
    format!("{base}{out}{RESET}")
}

/// Split `chars` into visual rows of at most `cw` columns. Rows are `[start, end)` char ranges;
/// a `\n` ends a row and is not part of any row.
pub fn wrap(chars: &[char], cw: usize) -> Vec<(usize, usize)> {
    let cw = cw.max(1);
    let mut rows = Vec::new();
    let (mut start, mut w) = (0usize, 0usize);
    let mut ended_by_newline = false;
    for (i, &c) in chars.iter().enumerate() {
        ended_by_newline = false;
        if c == '\n' {
            rows.push((start, i));
            start = i + 1;
            w = 0;
            ended_by_newline = true;
            continue;
        }
        let cwid = UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cwid > cw {
            rows.push((start, i));
            start = i;
            w = 0;
        }
        w += cwid;
    }
    rows.push((start, chars.len()));
    // Text that exactly fills the last row puts the cursor on a fresh row.
    if !ended_by_newline && !chars.is_empty() && w == cw {
        rows.push((chars.len(), chars.len()));
    }
    rows
}

pub(super) fn cursor_row(rows: &[(usize, usize)], cursor: usize) -> usize {
    rows.iter().rposition(|(s, _)| *s <= cursor).unwrap_or(0)
}

pub(super) fn style_row(
    chars: &[char],
    (s, e): (usize, usize),
    spans: &[(usize, usize, &'static str)],
    color: bool,
) -> String {
    if !color {
        return chars[s..e].iter().collect();
    }
    let mut out = String::new();
    let mut cur: &str = "";
    for (i, &c) in chars[s..e].iter().enumerate() {
        let idx = s + i;
        let want = spans
            .iter()
            .find(|(a, b, _)| idx >= *a && idx < *b)
            .map_or("", |(_, _, st)| *st);
        if want != cur {
            out.push_str(RESET);
            out.push_str(want);
            cur = want;
        }
        out.push(c);
    }
    if !cur.is_empty() {
        out.push_str(RESET);
    }
    out
}

pub(super) fn spans(
    text: &str,
    is_skill: &dyn Fn(&str) -> bool,
) -> Vec<(usize, usize, &'static str)> {
    let mut out: Vec<_> = expand::find_tokens(text)
        .into_iter()
        .filter_map(|t| match t.kind {
            TokKind::At => Some((t.start, t.end, CYAN)),
            TokKind::Dollar if is_skill(&t.value) => Some((t.start, t.end, YELLOW)),
            TokKind::Dollar => None,
        })
        .collect();
    let chars: Vec<char> = text.chars().collect();
    out.extend(
        crate::attach::image_tokens(&chars)
            .into_iter()
            .map(|(a, b, _)| (a, b, MAGENTA)),
    );
    if text.starts_with('/') {
        let end = text
            .chars()
            .position(char::is_whitespace)
            .unwrap_or(text.chars().count());
        out.push((0, end, GREEN));
    }
    out
}

pub(super) fn rule(width: usize, color: bool) -> String {
    let r = "─".repeat(width);
    if color { format!("{DIM}{r}{RESET}") } else { r }
}

pub(super) fn dim(s: &str, color: bool) -> String {
    style::paint(color, DIM, s)
}
