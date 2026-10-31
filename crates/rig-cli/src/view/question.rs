//! The question panel: the agent's question, its options, and the row for an answer of your own.

use super::text::{dim, truncate, width_of};
use crate::question::Question;
use crate::style::{BOLD, CYAN, RESET};

/// Rows of the panel, and the cursor. The cursor sits on the highlighted row, or after the typed
/// text when the person is typing an answer.
pub(super) fn rows(q: &Question, w: usize, color: bool) -> (Vec<String>, (usize, usize)) {
    let (bold, reset) = if color { (BOLD, RESET) } else { ("", "") };
    let mut lines = vec![format!(
        "{bold}{}{reset}",
        truncate(&format!("? {}", q.text), w)
    )];

    if let Some(text) = &q.typing {
        let line = truncate(&format!("  › {text}"), w);
        let cursor = (1, width_of(&line));
        lines.push(line);
        lines.push(dim(
            &truncate("  Enter send · Esc back · Ctrl-C skips the question", w),
            color,
        ));
        return (lines, cursor);
    }

    let cursor_row = q.selected;
    for (i, option) in q.options.iter().enumerate() {
        lines.push(choice_row(i, option, i == q.selected, w, color));
    }
    lines.push(choice_row(
        q.own_row(),
        "Type something…",
        q.selected == q.own_row(),
        w,
        color,
    ));
    lines.push(dim(
        &truncate("  ↑↓ move · Enter choose · 1-9 pick · Esc skip", w),
        color,
    ));
    (lines, (1 + cursor_row, 0))
}

fn choice_row(i: usize, label: &str, selected: bool, w: usize, color: bool) -> String {
    let marker = if selected { "›" } else { " " };
    let line = truncate(&format!("{marker} {}. {label}", i + 1), w);
    match (selected, color) {
        (true, true) => format!("{BOLD}{CYAN}{line}{RESET}"),
        _ => line,
    }
}
