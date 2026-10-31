//! The text box that replaces the composer while you type a key or a name.

use super::text::{dim, truncate, width_of};
use crate::entry::Entry;
use crate::style::{BOLD, RESET};

/// Rows of the panel, and the cursor, which rests after the typed text.
pub(super) fn rows(e: &Entry, w: usize, color: bool) -> (Vec<String>, (usize, usize)) {
    let (bold, reset) = if color { (BOLD, RESET) } else { ("", "") };
    let title = format!("{bold}{}{reset}", truncate(&e.title, w));
    let shown = if e.secret {
        "•".repeat(e.value.chars().count().min(w.saturating_sub(4)))
    } else {
        e.value.clone()
    };
    let line = truncate(&format!("  {shown}"), w);
    let cursor = (1, width_of(&line));
    let help = dim(
        &truncate(&format!("  {} · Enter save · Esc cancel", e.hint), w),
        color,
    );
    (vec![title, line, help], cursor)
}
