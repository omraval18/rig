//! The composer, with the completion popup under it.

use super::View;
use super::text::{cursor_row, dim, highlighted, rule, spans, style_row, truncate, width_of, wrap};
use crate::complete::{Completion, Kind, MAX_ROWS, Trigger};
use crate::style::{BOLD, CYAN, GREEN, RESET, YELLOW};
use rig_core::text::count;
use unicode_width::UnicodeWidthChar;

const PROMPT: &str = "› ";
const CONT: &str = "  ";
pub(super) const MAX_COMPOSER_ROWS: usize = 10;
const REVERSE: &str = "\x1b[7m";

/// Rows of the prompt box (composer, closing rule, popup) and the cursor within the composer.
pub(super) fn rows(v: &View, w: usize) -> (Vec<String>, (usize, usize)) {
    let (mut lines, cursor) = composer_rows(v, w);
    // The prompt is a box: rule above, rule below, and the status line under it.
    lines.push(rule(w, v.color));
    if let Some(c) = v.completion {
        lines.extend(completion_rows(c, w, v.color));
    }
    (lines, cursor)
}

fn composer_rows(v: &View, w: usize) -> (Vec<String>, (usize, usize)) {
    let chars = v.composer.chars();
    let text: String = chars.iter().collect();
    let tokens = spans(&text, v.is_skill);
    let rows = wrap(chars, w.saturating_sub(width_of(PROMPT)));
    let crow = cursor_row(&rows, v.composer.cursor);
    let first = crow
        .saturating_sub(MAX_COMPOSER_ROWS - 1)
        .min(rows.len().saturating_sub(MAX_COMPOSER_ROWS));
    let last = (first + MAX_COMPOSER_ROWS).min(rows.len());

    let mut lines = Vec::new();
    if first > 0 {
        let more = count(first, "more line");
        lines.push(dim(&format!("  ↑ {more}"), v.color));
    }
    let base = lines.len();
    for (n, r) in rows.iter().enumerate().take(last).skip(first) {
        let prefix = match (n, v.color) {
            (0, true) => format!("{BOLD}{PROMPT}{RESET}"),
            (0, false) => PROMPT.to_string(),
            _ => CONT.to_string(),
        };
        lines.push(format!(
            "{prefix}{}",
            style_row(chars, *r, &tokens, v.color)
        ));
    }
    let (row_start, _) = rows[crow];
    let col: usize = chars[row_start..v.composer.cursor.min(chars.len())]
        .iter()
        .map(|c| UnicodeWidthChar::width(*c).unwrap_or(0))
        .sum();
    (lines, (base + (crow - first), width_of(PROMPT) + col))
}

fn completion_rows(c: &Completion, w: usize, color: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if c.items.is_empty() {
        let msg = if c.loading {
            "  indexing workspace…"
        } else {
            "  no matches"
        };
        lines.push(dim(msg, color));
        return lines;
    }
    let start = (c.selected + 1).saturating_sub(MAX_ROWS);
    for (n, it) in c.items.iter().enumerate().skip(start).take(MAX_ROWS) {
        lines.push(item_row(it, n == c.selected, w, color));
    }
    lines.push(dim(&truncate(hint(&c.trigger), w), color));
    lines
}

fn item_row(it: &crate::complete::Item, selected: bool, w: usize, color: bool) -> String {
    let tint = match it.kind {
        Kind::Dir => CYAN,
        Kind::Command => GREEN,
        Kind::Skill => YELLOW,
        _ => "",
    };
    let label_room = if it.detail.is_empty() {
        w.saturating_sub(4)
    } else {
        (w * 2 / 5).max(14).min(w.saturating_sub(8))
    };
    let label = highlighted(
        &it.label,
        &it.positions,
        if color { tint } else { "" },
        if color { BOLD } else { "" },
        label_room,
        color,
    );
    let detail = if it.detail.is_empty() {
        String::new()
    } else {
        let shown = width_of(&truncate(&it.label, label_room));
        let pad = " ".repeat(label_room.saturating_sub(shown).min(40) + 2);
        let text = truncate(&it.detail, w.saturating_sub(label_room + 8));
        format!("{pad}{}", dim(&text, color))
    };
    let line = format!("{} {label}{detail}", if selected { "›" } else { " " });
    if selected && color {
        format!("{REVERSE}{RESET}{BOLD}{line}{RESET}")
    } else {
        line
    }
}

fn hint(trigger: &Trigger) -> &'static str {
    match trigger {
        Trigger::Slash { .. } => "  ↑↓ move · Enter run · Tab complete · Esc close",
        Trigger::At { .. } => {
            "  ↑↓ move · Enter mention (folders too) · Tab open folder · Esc close"
        }
        Trigger::Dollar { .. } => "  ↑↓ move · Enter/Tab use skill · Esc close",
        Trigger::Arg { .. } => "  ↑↓ move · Enter apply · Tab fill · Esc close",
    }
}
