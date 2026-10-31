//! Layout of the live region at the bottom of the terminal: activity line, prompt,
//! completion popup / picker / approval panel, and the footer.
//!
//! Pure functions from state to rows. Every returned row is at most `width` columns
//! wide, so the screen writer can move the cursor with plain row arithmetic.

use crate::complete::Completion;
use crate::composer::Composer;
use crate::entry::Entry;
use crate::picker::Picker;

mod approval;
mod entry;
mod footer;
mod picker;
mod prompt;
mod text;

use footer::footer_line;
use text::{dim, rule};

pub use text::{truncate, width_of, wrap};

pub struct ApprovalView {
    pub title: String,
    pub detail: String,
    pub reason: String,
    /// Label for the "always" choice, e.g. `Always allow "run cargo test"`.
    pub always: String,
    pub selected: usize,
}

/// A piece of the footer: text and the colour it is drawn in (empty = the footer's dim default).
pub type Seg = (String, &'static str);

pub struct View<'a> {
    pub width: usize,
    pub color: bool,
    pub composer: &'a Composer,
    pub completion: Option<&'a Completion>,
    pub picker: Option<&'a Picker>,
    pub entry: Option<&'a Entry>,
    pub approval: Option<&'a ApprovalView>,
    /// Unfinished line of assistant text, shown live until its newline arrives.
    pub partial: &'a str,
    /// Tools that are running right now, already styled (pulsing dot included), one row each.
    pub tool_rows: Vec<String>,
    pub activity: Option<String>,
    pub notice: Option<String>,
    pub footer_left: Vec<Seg>,
    pub footer_right: Vec<Seg>,
    pub is_skill: &'a dyn Fn(&str) -> bool,
}

pub struct Frame {
    pub lines: Vec<String>,
    /// (row, column) of the cursor within `lines`.
    pub cursor: (usize, usize),
}

pub fn render(v: &View) -> Frame {
    let w = v.width.max(20);
    // A blank row separates the live region from the transcript above it.
    let mut lines = vec![String::new()];
    lines.extend(live_rows(v, w));
    lines.push(rule(w, v.color));

    // Each panel reports its cursor relative to its own first row.
    let (panel, (row, col)) = if let Some(a) = v.approval {
        approval::rows(a, w, v.color)
    } else if let Some(p) = v.picker {
        picker::rows(p, w, v.color)
    } else if let Some(e) = v.entry {
        entry::rows(e, w, v.color)
    } else {
        prompt::rows(v, w)
    };
    let cursor = (lines.len() + row, col);
    lines.extend(panel);

    // Panels that replace the composer are closed by a rule; the composer's own rule comes first.
    if v.approval.is_some() || v.picker.is_some() || v.entry.is_some() {
        lines.push(rule(w, v.color));
    }
    lines.push(footer_line(&v.footer_left, &v.footer_right, w, v.color));
    Frame { lines, cursor }
}

/// What is going on above the prompt: streaming text, running tools, activity and notices.
fn live_rows(v: &View, w: usize) -> Vec<String> {
    let mut lines = Vec::new();
    // Streaming text that has not reached a newline yet.
    let chars: Vec<char> = v.partial.chars().collect();
    if !chars.is_empty() {
        for (s, e) in wrap(&chars, w) {
            lines.push(chars[s..e].iter().collect());
        }
    }
    lines.extend(v.tool_rows.iter().cloned());
    for line in [&v.activity, &v.notice].into_iter().flatten() {
        lines.push(dim(&truncate(line, w), v.color));
    }
    lines
}

#[cfg(test)]
mod tests;
