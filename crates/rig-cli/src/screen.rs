//! The terminal as a grid of rows. rig paints the whole window and only rewrites rows that changed,
//! so there is no relative cursor arithmetic to go wrong when the window is resized, and the cursor
//! is hidden while painting so it never flickers across the screen.
//!
//! The writer is generic over `Write` so the escape sequences can be inspected in tests.

use crate::ansi;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::queue;
use crossterm::style::Print;
use crossterm::terminal::{Clear, ClearType};
use std::io::{self, Write};

pub struct Screen<W: Write> {
    out: W,
    /// What is on the terminal now, row by row.
    shown: Vec<String>,
}

impl<W: Write> Screen<W> {
    pub fn new(out: W) -> Self {
        Self {
            out,
            shown: Vec::new(),
        }
    }

    /// Forget what is on screen: the next paint rewrites everything (after a resize, or Ctrl-L).
    pub fn invalidate(&mut self) -> io::Result<()> {
        self.shown.clear();
        queue!(self.out, Clear(ClearType::All))?;
        self.out.flush()
    }

    /// Show `rows` (one per terminal row, `width` columns wide) with the cursor at `cursor` (row, column).
    pub fn paint(
        &mut self,
        rows: &[String],
        width: usize,
        cursor: Option<(usize, usize)>,
    ) -> io::Result<()> {
        // Synchronized output: terminals that support it show the update atomically.
        queue!(self.out, Print("\x1b[?2026h"), Hide)?;
        if self.shown.len() != rows.len() {
            self.shown.clear();
            queue!(self.out, Clear(ClearType::All))?;
        }
        for (y, row) in rows.iter().enumerate() {
            let row = ansi::clip(row, width);
            if self.shown.get(y) == Some(&row) {
                continue;
            }
            queue!(
                self.out,
                MoveTo(0, y as u16),
                Print(&row),
                Print("\x1b[0m"),
                Clear(ClearType::UntilNewLine)
            )?;
            if y < self.shown.len() {
                self.shown[y] = row;
            } else {
                self.shown.push(row);
            }
        }
        if let Some((y, x)) = cursor {
            queue!(self.out, MoveTo(x as u16, y as u16), Show)?;
        }
        queue!(self.out, Print("\x1b[?2026l"))?;
        self.out.flush()
    }
}

#[cfg(test)]
mod tests;
