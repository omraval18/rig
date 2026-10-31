//! Width-aware handling of styled text: wrapping at word boundaries and clipping, with colours
//! carried across rows. Escape sequences take no columns.

use unicode_width::UnicodeWidthChar;

const RESET: &str = "\x1b[0m";

#[derive(Clone)]
enum Piece {
    /// An escape sequence, passed through untouched.
    Esc(String),
    Ch(char, usize),
}

fn pieces(s: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            let mut seq = String::from(c);
            if it.peek() == Some(&'[') {
                seq.extend(it.next());
                while let Some(&n) = it.peek() {
                    seq.push(n);
                    it.next();
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            out.push(Piece::Esc(seq));
        } else {
            out.push(Piece::Ch(c, UnicodeWidthChar::width(c).unwrap_or(0)));
        }
    }
    out
}

fn is_reset(e: &str) -> bool {
    e == RESET || e == "\x1b[m"
}

/// Columns the text occupies on screen.
pub fn visible_width(s: &str) -> usize {
    pieces(s)
        .iter()
        .map(|p| if let Piece::Ch(_, w) = p { *w } else { 0 })
        .sum()
}

/// Cut to at most `w` columns, closing any open style.
pub fn clip(s: &str, w: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let (mut col, mut styled, mut cut) = (0, false, false);
    for p in pieces(s) {
        match p {
            Piece::Esc(e) => {
                if e.ends_with('m') {
                    styled = !is_reset(&e);
                }
                out.push_str(&e);
            }
            Piece::Ch(c, cw) if !cut => {
                if col + cw > w {
                    cut = true;
                } else {
                    out.push(c);
                    col += cw;
                }
            }
            Piece::Ch(..) => {}
        }
    }
    if styled {
        out.push_str(RESET);
    }
    out
}

/// How far continuation rows are indented: the line's own indent plus a list marker's width.
fn hanging_indent(ps: &[Piece]) -> usize {
    let text: String = ps
        .iter()
        .filter_map(|p| {
            if let Piece::Ch(c, _) = p {
                Some(*c)
            } else {
                None
            }
        })
        .collect();
    let rest = text.trim_start();
    let lead = text.chars().count() - rest.chars().count();
    let marker = if rest.starts_with("- ")
        || rest.starts_with("* ")
        || rest.starts_with("• ")
        || rest.starts_with("◦ ")
    {
        2
    } else {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && rest[digits..].starts_with(". ") {
            digits + 2
        } else {
            0
        }
    };
    (lead + marker).min(12)
}

/// `prefix`, the styles active at the row's start, the row, and a reset if a style is still open.
/// Also returns the styles in force at the end, for the next row.
fn render_row(prefix: &str, start: &str, row: &[Piece]) -> (String, String) {
    let mut line = String::from(prefix);
    line.push_str(start);
    let mut st = start.to_string();
    for p in row {
        match p {
            Piece::Esc(e) => {
                if e.ends_with('m') {
                    if is_reset(e) {
                        st.clear();
                    } else {
                        st.push_str(e);
                    }
                }
                line.push_str(e);
            }
            Piece::Ch(c, _) => line.push(*c),
        }
    }
    if !st.is_empty() {
        line.push_str(RESET);
    }
    (line, st)
}

fn width_of(row: &[Piece]) -> usize {
    row.iter()
        .map(|p| if let Piece::Ch(_, w) = p { *w } else { 0 })
        .sum()
}

/// Break a styled line into rows of at most `w` columns, at spaces where possible. Each row starts
/// with the styles that were active where the previous one ended, and continuation rows keep a
/// list item's indent.
pub fn wrap(s: &str, w: usize) -> Vec<String> {
    let w = w.max(4);
    if visible_width(s) <= w {
        return vec![s.to_string()];
    }
    let ps = pieces(s);
    let indent = " ".repeat(hanging_indent(&ps).min(w / 2));
    let (mut rows, mut row, mut col) = (Vec::new(), Vec::<Piece>::new(), 0usize);
    let (mut start, mut first) = (String::new(), true);
    for p in ps {
        let cw = if let Piece::Ch(_, cw) = &p { *cw } else { 0 };
        let room = if first { w } else { w - indent.len() };
        if cw > 0 && col + cw > room {
            let brk = row.iter().rposition(|q| matches!(q, Piece::Ch(' ', _)));
            let (head, tail) = match brk {
                Some(b) if b > 0 => (row[..b].to_vec(), row[b + 1..].to_vec()),
                _ => (row.clone(), Vec::new()),
            };
            let (line, end) = render_row(if first { "" } else { &indent }, &start, &head);
            rows.push(line);
            start = end;
            first = false;
            col = width_of(&tail);
            row = tail;
        }
        col += cw;
        row.push(p);
    }
    let (line, _) = render_row(if first { "" } else { &indent }, &start, &row);
    rows.push(line);
    rows
}

#[cfg(test)]
mod tests;
