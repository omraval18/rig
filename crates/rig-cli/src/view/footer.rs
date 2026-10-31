//! The status line under the prompt.

use super::*;
use crate::style;

pub(super) fn seg(text: &str, style: &str, color: bool) -> String {
    // A coloured segment ends with RESET, which also ends the footer's dim: put it back.
    if color && !style.is_empty() {
        format!(
            "{}{}{text}{}{}",
            style::RESET,
            style,
            style::RESET,
            style::DIM
        )
    } else {
        text.to_string()
    }
}

/// Footer: `left` fits first (cut from the right when short on room), `right` is pinned to the edge.
pub(super) fn footer_line(left: &[Seg], right: &[Seg], w: usize, color: bool) -> String {
    let rw: usize = right.iter().map(|(t, _)| width_of(t)).sum();
    let right_fits = rw + 4 < w;
    let room = if right_fits { w - rw - 2 } else { w };
    let (mut used, mut l) = (0, String::new());
    for (text, st) in left {
        let t = truncate(text, room - used);
        used += width_of(&t);
        l.push_str(&seg(&t, st, color));
        if t != *text {
            break;
        }
    }
    let mut r = String::new();
    if right_fits {
        l.push_str(&" ".repeat(w - used - rw));
        for (text, st) in right {
            r.push_str(&seg(text, st, color));
        }
    }
    if color {
        format!("{}{l}{r}{}", style::DIM, style::RESET)
    } else {
        format!("{l}{r}")
    }
}
