//! The filter-as-you-type list that replaces the composer for `/model`, `/resume` and friends.

use super::text::{dim, highlighted, truncate, width_of};
use crate::complete::MAX_ROWS;
use crate::picker::{PickItem, PickKind, Picker};
use crate::style;
use crate::style::{BOLD, RESET};

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
    lines.push(filter_line);

    let rows = p.rows(MAX_ROWS);
    if rows.is_empty() {
        lines.push(dim("  no matches", color));
    }
    let label_room = (w / 2).max(12);
    // One detail column for the whole list, so descriptions line up.
    let col = rows
        .iter()
        .map(|(it, _, _)| {
            width_of(&truncate(&it.label, label_room)) + if it.current { 2 } else { 0 }
        })
        .max()
        .unwrap_or(0);
    for (item, positions, selected) in rows {
        let line = item_row(
            p.kind,
            item,
            positions,
            selected,
            (w, label_room, col),
            color,
        );
        lines.push(line);
    }
    lines.push(dim(
        &truncate("  type to filter · ↑↓ move · Enter select · Esc cancel", w),
        color,
    ));
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
    let gap = " ".repeat(col + 2 - used);
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
