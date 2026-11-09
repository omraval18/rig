//! Mouse selection over transcript rows, including timed scrolling at viewport edges.

use crate::{ansi, transcript::Transcript};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Point {
    row: usize,
    col: usize,
}

pub(super) struct Selection {
    anchor: Point,
    head: Point,
    dragging: bool,
    moved: bool,
    edge: isize,
    column: usize,
    last_scroll: Instant,
}

impl Selection {
    pub fn start(transcript: &Transcript, height: usize, row: usize, col: usize) -> Option<Self> {
        let point = Point {
            row: transcript.row_at(height, row)?,
            col,
        };
        Some(Self {
            anchor: point,
            head: point,
            dragging: true,
            moved: false,
            edge: 0,
            column: col,
            last_scroll: Instant::now(),
        })
    }

    pub fn drag(&mut self, transcript: &Transcript, height: usize, row: usize, col: usize) {
        if !self.dragging || height == 0 {
            return;
        }
        self.column = col;
        self.edge = if row == 0 {
            1
        } else if row >= height - 1 {
            -1
        } else {
            0
        };
        if let Some(row) = transcript.row_at(height, row.min(height - 1)) {
            self.head = Point { row, col };
            self.moved |= self.head != self.anchor;
        }
    }

    pub fn tick(&mut self, transcript: &mut Transcript, height: usize) -> bool {
        if !self.dragging
            || self.edge == 0
            || height == 0
            || self.last_scroll.elapsed() < Duration::from_millis(80)
        {
            return false;
        }
        self.last_scroll = Instant::now();
        let before = transcript.scroll;
        transcript.scroll_by(self.edge, height);
        if before == transcript.scroll {
            return false;
        }
        let row = if self.edge > 0 { 0 } else { height - 1 };
        if let Some(row) = transcript.row_at(height, row) {
            self.head = Point {
                row,
                col: self.column,
            };
            self.moved |= self.head != self.anchor;
        }
        true
    }

    pub fn finish(&mut self) -> bool {
        self.dragging = false;
        self.moved
    }

    fn bounds(&self) -> (Point, Point) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    fn range(&self, row: usize) -> Option<(usize, usize)> {
        if !self.moved {
            return None;
        }
        let (start, end) = self.bounds();
        (row >= start.row && row <= end.row).then_some((
            if row == start.row { start.col } else { 0 },
            if row == end.row {
                end.col.saturating_add(1)
            } else {
                usize::MAX
            },
        ))
    }

    pub fn paint(&self, row: usize, text: &str) -> String {
        self.range(row)
            .map_or_else(|| text.to_string(), |(a, b)| ansi::highlight(text, a, b))
    }

    pub fn text(&self, transcript: &Transcript) -> String {
        let (start, end) = self.bounds();
        (start.row..=end.row)
            .filter_map(|row| {
                let (a, b) = self.range(row)?;
                Some(
                    ansi::columns(transcript.row(row)?, a, b)
                        .trim_end()
                        .to_string(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_in_both_directions_and_strips_styles() {
        let mut transcript = Transcript::new(40, false);
        transcript.push_text("first\n\x1b[31msecond\x1b[0m\nthird");
        let mut selection = Selection::start(&transcript, 3, 0, 2).unwrap();
        selection.drag(&transcript, 3, 2, 2);
        assert_eq!(selection.text(&transcript), "rst\nsecond\nthi");
        assert!(selection.finish());
        let mut reverse = Selection::start(&transcript, 3, 2, 2).unwrap();
        reverse.drag(&transcript, 3, 0, 2);
        assert_eq!(reverse.text(&transcript), selection.text(&transcript));
    }

    #[test]
    fn holding_at_both_edges_scrolls_and_extends_selection() {
        let mut transcript = Transcript::new(40, false);
        transcript.push_text("zero\none\ntwo\nthree\nfour\nfive");
        transcript.scroll_by(2, 3);
        let mut selection = Selection::start(&transcript, 3, 1, 0).unwrap();
        selection.drag(&transcript, 3, 2, 1);
        selection.last_scroll -= Duration::from_secs(1);
        assert!(selection.tick(&mut transcript, 3));
        assert_eq!(transcript.scroll, 1);
        assert_eq!(selection.text(&transcript), "two\nthree\nfo");
        selection.drag(&transcript, 3, 0, 0);
        selection.last_scroll -= Duration::from_secs(1);
        assert!(selection.tick(&mut transcript, 3));
        assert_eq!(transcript.scroll, 2);
        assert_eq!(selection.text(&transcript), "one\nt");
        selection.finish();
        selection.last_scroll -= Duration::from_secs(1);
        assert!(!selection.tick(&mut transcript, 3));
    }
}
