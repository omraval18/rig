//! The prompt editor's state: text, cursor, history and kill ring. No terminal I/O.
//!
//! The cursor is a char index. Lines are separated by `\n`.

#[derive(Default)]
pub struct Composer {
    chars: Vec<char>,
    pub cursor: usize,
    history: Vec<String>,
    /// Position while browsing history (`history.len()` = the draft).
    browsing: Option<usize>,
    draft: String,
    kill: String,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Composer {
    pub fn new(history: Vec<String>) -> Self {
        Self {
            history,
            ..Self::default()
        }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn chars(&self) -> &[char] {
        &self.chars
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn set_text(&mut self, s: &str) {
        self.chars = s.chars().collect();
        self.cursor = self.chars.len();
        self.browsing = None;
    }

    pub fn clear(&mut self) {
        self.set_text("");
    }

    pub fn insert_str(&mut self, s: &str) {
        let clean: Vec<char> = s
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .chars()
            .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
            .collect();
        let n = clean.len();
        self.chars.splice(self.cursor..self.cursor, clean);
        self.cursor += n;
        self.browsing = None;
    }

    pub fn insert_char(&mut self, c: char) {
        self.insert_str(&c.to_string());
    }

    pub fn newline(&mut self) {
        self.insert_str("\n");
    }

    /// Replace `start..end` with `with` and put the cursor after it.
    pub fn replace_range(&mut self, start: usize, end: usize, with: &str) {
        let w: Vec<char> = with.chars().collect();
        let n = w.len();
        self.chars.splice(start..end.min(self.chars.len()), w);
        self.cursor = start + n;
        self.browsing = None;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
            self.browsing = None;
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
            self.browsing = None;
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.chars.len());
    }

    fn line_start(&self) -> usize {
        self.chars[..self.cursor]
            .iter()
            .rposition(|c| *c == '\n')
            .map_or(0, |i| i + 1)
    }

    fn line_end(&self) -> usize {
        self.chars[self.cursor..]
            .iter()
            .position(|c| *c == '\n')
            .map_or(self.chars.len(), |i| self.cursor + i)
    }

    pub fn home(&mut self) {
        self.cursor = self.line_start();
    }

    pub fn end(&mut self) {
        self.cursor = self.line_end();
    }

    pub fn word_left(&mut self) {
        while self.cursor > 0 && !is_word(self.chars[self.cursor - 1]) {
            self.cursor -= 1;
        }
        while self.cursor > 0 && is_word(self.chars[self.cursor - 1]) {
            self.cursor -= 1;
        }
    }

    pub fn word_right(&mut self) {
        while self.cursor < self.chars.len() && !is_word(self.chars[self.cursor]) {
            self.cursor += 1;
        }
        while self.cursor < self.chars.len() && is_word(self.chars[self.cursor]) {
            self.cursor += 1;
        }
    }

    pub fn kill_to_end(&mut self) {
        let end = if self.line_end() == self.cursor && self.cursor < self.chars.len() {
            self.cursor + 1
        } else {
            self.line_end()
        };
        self.kill = self.chars.drain(self.cursor..end).collect();
        self.browsing = None;
    }

    pub fn kill_to_start(&mut self) {
        // At the start of a line, join it to the previous one so repeated Ctrl-U clears a multi-line draft.
        let start = if self.cursor > 0 && self.cursor == self.line_start() {
            self.cursor - 1
        } else {
            self.line_start()
        };
        self.kill = self.chars.drain(start..self.cursor).collect();
        self.cursor = start;
        self.browsing = None;
    }

    pub fn kill_word_back(&mut self) {
        let end = self.cursor;
        self.word_left();
        self.kill = self.chars.drain(self.cursor..end).collect();
        self.browsing = None;
    }

    pub fn yank(&mut self) {
        let k = self.kill.clone();
        self.insert_str(&k);
    }

    /// Move up one logical line. `false` if already on the first line.
    pub fn up(&mut self) -> bool {
        let start = self.line_start();
        if start == 0 {
            return false;
        }
        let col = self.cursor - start;
        let prev_end = start - 1;
        let prev_start = self.chars[..prev_end]
            .iter()
            .rposition(|c| *c == '\n')
            .map_or(0, |i| i + 1);
        self.cursor = prev_start + col.min(prev_end - prev_start);
        true
    }

    /// Move down one logical line. `false` if already on the last line.
    pub fn down(&mut self) -> bool {
        let end = self.line_end();
        if end >= self.chars.len() {
            return false;
        }
        let col = self.cursor - self.line_start();
        let next_start = end + 1;
        let next_end = self.chars[next_start..]
            .iter()
            .position(|c| *c == '\n')
            .map_or(self.chars.len(), |i| next_start + i);
        self.cursor = next_start + col.min(next_end - next_start);
        true
    }

    pub fn history_prev(&mut self) -> bool {
        if self.history.is_empty() {
            return false;
        }
        let i = match self.browsing {
            None => {
                self.draft = self.text();
                self.history.len() - 1
            }
            Some(0) => return false,
            Some(i) => i - 1,
        };
        self.show_history(i);
        true
    }

    pub fn history_next(&mut self) -> bool {
        let Some(i) = self.browsing else { return false };
        if i + 1 >= self.history.len() {
            let d = std::mem::take(&mut self.draft);
            self.set_text(&d);
        } else {
            self.show_history(i + 1);
        }
        true
    }

    fn show_history(&mut self, i: usize) {
        let t = self.history[i].clone();
        self.chars = t.chars().collect();
        self.cursor = self.chars.len();
        self.browsing = Some(i);
    }

    /// Take the text for sending and remember it in history.
    pub fn submit(&mut self) -> String {
        let t = self.text().trim_end().to_string();
        if !t.trim().is_empty() && self.history.last() != Some(&t) {
            self.history.push(t.clone());
        }
        self.chars.clear();
        self.cursor = 0;
        self.browsing = None;
        t
    }

    /// True when the cursor line ends with an unescaped trailing backslash (line continuation).
    pub fn continues_line(&self) -> bool {
        self.cursor > 0 && self.cursor == self.line_end() && self.chars[self.cursor - 1] == '\\'
    }
}

#[cfg(test)]
mod tests;
