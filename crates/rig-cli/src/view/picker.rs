//! The filter-as-you-type list that replaces the composer for `/model`, `/resume` and friends.

use super::text::{dim, highlighted, truncate, width_of};
use crate::picker::{PickItem, PickKind, Picker, Row};
use crate::style;
use crate::style::{BOLD, RESET};

/// Lines of the list, headings included. With the panel around it this fits a 24-row window.
const MAX_ROWS: usize = 14;

/// Rows of the panel, and the cursor, which rests at the end of the filter text.
pub(super) fn rows(p: &Picker, w: usize, color: bool) -> (Vec<String>, (usize, usize)) {
    let bold = if color { BOLD } else { "" };
    let mut lines = vec![format!(
        "{bold}{}{}",
        truncate(&p.title, w),
        if color { RESET } else { "" }
    )];
    let filter_line = truncate(&format!("  filter: {}", p.filter), w);
    let cursor = (1, width_of(&filter_line));
    let position = p.position();
    let gap = w.saturating_sub(width_of(&filter_line) + width_of(&position));
    lines.push(if gap > 1 {
        format!("{filter_line}{}{}", " ".repeat(gap), dim(&position, color))
    } else {
        filter_line
    });

    let rows = p.rows(MAX_ROWS);
    if rows.is_empty() {
        lines.push(dim("  no matches", color));
    }
    let label_room = (w / 2).max(12);
    // One detail column for the whole list, so descriptions line up.
    let col = rows
        .iter()
        .filter_map(|r| match r {
            Row::Item { item, .. } => Some(
                width_of(&truncate(&item.label, label_room)) + if item.current { 2 } else { 0 },
            ),
            Row::Heading(_) => None,
        })
        .max()
        .unwrap_or(0);
    for row in rows {
        lines.push(match row {
            Row::Heading(text) => {
                let text = truncate(&format!("  {text}"), w);
                style::paint(color, style::BOLD, &dim(&text, color))
            }
            Row::Item {
                item,
                positions,
                selected,
            } => item_row(
                p.kind,
                item,
                positions,
                selected,
                (w, label_room, col),
                color,
            ),
        });
    }
    lines.push(dim(&truncate(&format!("  {}", p.kind.hint()), w), color));
    (lines, cursor)
}

/// `layout` is `(width, label room, detail column)`.
fn item_row(
    kind: PickKind,
    item: &PickItem,
    positions: &[usize],
    selected: bool,
    (w, label_room, col): (usize, usize, usize),
    color: bool,
) -> String {
    let marker = if selected { "›" } else { " " };
    let current = if item.current { " ●" } else { "" };
    let label = highlighted(
        &item.label,
        positions,
        item_color(kind, item),
        if color { BOLD } else { "" },
        label_room,
        color,
    );
    let used = width_of(&truncate(&item.label, label_room)) + width_of(current);
    let gap = " ".repeat((col + 2).saturating_sub(used));
    let detail_text = format!(
        "{gap}{}",
        truncate(&item.detail, w.saturating_sub(3 + col + 2))
    );
    let detail = if item.detail.is_empty() {
        String::new()
    } else if kind == PickKind::Settings {
        detail_text
    } else {
        dim(&detail_text, color)
    };
    let line = format!(
        "{marker} {label}{}{detail}",
        style::paint(color, style::GREEN, current)
    );
    if selected && color {
        format!("{BOLD}{line}{RESET}")
    } else {
        line
    }
}

/// Picker labels carry meaning: permission levels by how much they allow, efforts and models by kind.
fn item_color(kind: PickKind, item: &PickItem) -> &'static str {
    match kind {
        PickKind::Permissions => {
            rig_core::permission::Autonomy::parse(&item.value).map_or("", style::permission)
        }
        PickKind::Effort => style::VIOLET,
        PickKind::Model => style::CYAN,
        _ => "",
    }
}
