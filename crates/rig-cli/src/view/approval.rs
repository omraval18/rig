//! The permission prompt, shown in place of the composer.

use super::ApprovalView;
use super::text::{dim, truncate, width_of};
use crate::style;
use crate::style::{BOLD, RESET};

/// Reverse video turns an option's colour into its background.
const REVERSE: &str = "\x1b[7m";

/// Rows of the panel, and the cursor, which rests on the choices row.
pub(super) fn rows(a: &ApprovalView, w: usize, color: bool) -> (Vec<String>, (usize, usize)) {
    let title = style::paint(
        color,
        &format!("{}{}", style::YELLOW, style::BOLD),
        &truncate(&format!("? {}", a.title), w),
    );
    let reason = style::paint(
        color,
        style::risk_in(&a.reason),
        &truncate(&format!("  {}", a.reason), w),
    );
    let help = dim(
        &truncate(
            "  ←/→ choose · Enter confirm · y once · a always · n or Esc no",
            w,
        ),
        color,
    );
    let lines = vec![
        title,
        detail_row(&a.detail, w, color),
        reason,
        choices_row(a, color),
        help,
    ];
    (lines, (3, 2))
}

/// The command stands out from the one-line summary that precedes it.
fn detail_row(detail: &str, w: usize, color: bool) -> String {
    match detail.split_once("  $ ") {
        Some((what, cmd)) if color => format!(
            "  {what}  {}",
            style::paint(
                true,
                style::CYAN,
                &truncate(&format!("$ {cmd}"), w.saturating_sub(width_of(what) + 4))
            )
        ),
        _ => truncate(&format!("  {detail}"), w),
    }
}

fn choices_row(a: &ApprovalView, color: bool) -> String {
    let options = [
        ("Yes, once", style::GREEN),
        (a.always.as_str(), style::BLUE),
        ("No", style::RED),
    ];
    let mut row = String::from("  ");
    for (i, (option, tint)) in options.iter().enumerate() {
        let label = format!(" {option} ");
        if i == a.selected && color {
            row.push_str(&format!("{REVERSE}{tint}{BOLD}{label}{RESET}"));
        } else if i == a.selected {
            row.push_str(&format!("[{option}]"));
        } else {
            row.push_str(&style::paint(color, tint, &label));
        }
        row.push(' ');
    }
    row
}
