//! The conversation as the screen shows it: a list of entries wrapped to the window width, with a
//! scroll position. rig owns the scrollback (the UI runs full-screen), so resizing re-wraps
//! everything and `/new` simply empties it.

use crate::ansi;
use crate::style;

/// Background and foreground of your own messages: a card that reads on dark and light terminals.
const USER_BG: &str = "\x1b[48;5;237m\x1b[38;5;255m";
const USER_MARK: &str = "\x1b[38;5;246m";

pub enum Entry {
    /// One logical line of styled text.
    Text(String),
    /// What you typed, drawn as a shaded card.
    Card(String),
    Gap,
}

pub struct Transcript {
    entries: Vec<Entry>,
    /// `entries` wrapped to `width`, one screen row each.
    rows: Vec<String>,
    width: usize,
    color: bool,
    /// Rows scrolled up from the bottom; 0 follows the newest output.
    pub scroll: usize,
}

impl Transcript {
    pub fn new(width: usize, color: bool) -> Self {
        Transcript {
            entries: Vec::new(),
            rows: Vec::new(),
            width: width.max(20),
            color,
            scroll: 0,
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.rows.clear();
        self.scroll = 0;
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn render(&self, e: &Entry) -> Vec<String> {
        match e {
            Entry::Text(t) => ansi::wrap(t, self.width),
            Entry::Card(t) => user_card(t, self.width, self.color)
                .split('\n')
                .map(String::from)
                .collect(),
            Entry::Gap => vec![String::new()],
        }
    }

    fn push(&mut self, e: Entry) {
        let rows = self.render(&e);
        // Reading older output: new lines arrive below without moving what you are looking at.
        if self.scroll > 0 {
            self.scroll += rows.len();
        }
        self.rows.extend(rows);
        self.entries.push(e);
    }

    /// Append text; each `\n` starts a new line.
    pub fn push_text(&mut self, text: &str) {
        for line in text.split('\n') {
            self.push(Entry::Text(line.to_string()));
        }
    }

    pub fn push_card(&mut self, text: &str) {
        self.push(Entry::Card(text.to_string()));
    }

    pub fn push_gap(&mut self) {
        self.push(Entry::Gap);
    }

    pub fn set_width(&mut self, width: usize) {
        let width = width.max(20);
        if width == self.width {
            return;
        }
        self.width = width;
        let rows: Vec<String> = self.entries.iter().flat_map(|e| self.render(e)).collect();
        self.rows = rows;
        self.scroll = self.scroll.min(self.rows.len());
    }

    #[cfg(test)]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Scroll by `delta` rows (positive = towards older output) within a viewport of `height` rows.
    pub fn scroll_by(&mut self, delta: isize, height: usize) {
        let max = self.rows.len().saturating_sub(height);
        self.scroll = (self.scroll as isize + delta).clamp(0, max as isize) as usize;
    }

    /// The rows to show in a viewport of `height`, top-aligned, padded with blanks.
    pub fn view(&mut self, height: usize) -> Vec<String> {
        let max = self.rows.len().saturating_sub(height);
        self.scroll = self.scroll.min(max);
        let end = self.rows.len() - self.scroll;
        let start = end.saturating_sub(height);
        let mut out: Vec<String> = self.rows[start..end].to_vec();
        out.resize(height, String::new());
        out
    }
}

/// Render `text` as a card `width` columns wide: one column of padding either side of the "›" mark.
pub fn user_card(text: &str, width: usize, color: bool) -> String {
    let w = width.max(20);
    let chars: Vec<char> = text
        .trim_end()
        .replace('\t', "    ")
        .replace('\r', "")
        .chars()
        .collect();
    let rows = crate::view::wrap(&chars, w.saturating_sub(4));
    let mut lines = Vec::with_capacity(rows.len());
    for (i, (a, b)) in rows.into_iter().enumerate() {
        let body: String = chars[a..b].iter().collect();
        let mark = if i == 0 { "›" } else { " " };
        if !color {
            lines.push(format!("{mark} {body}"));
            continue;
        }
        let pad = " ".repeat(w.saturating_sub(3 + crate::view::width_of(&body)));
        lines.push(format!(
            "{USER_BG} {USER_MARK}{mark}\x1b[38;5;255m {body}{pad}{}",
            style::RESET
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
