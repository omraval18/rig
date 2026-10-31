//! Printing into the transcript and the small styling helpers that go with it.

use super::format::tool_label;
use super::{Part, Tui};
use crate::style;
use crate::view::{self};
use std::time::{Duration, Instant};

impl Tui {
    /// Print a transcript block of `kind`, preceded by one blank line when the kind changes.
    pub(super) fn print_part(&mut self, kind: Part, text: &str) {
        self.begin_part(kind);
        self.display.transcript.push_text(text);
        self.dirty = true;
    }

    pub(super) fn begin_part(&mut self, kind: Part) {
        if self.display.last_part != kind && self.display.last_part != Part::Start {
            self.display.transcript.push_gap();
        }
        self.display.last_part = kind;
    }

    /// Command output and other notes.
    pub fn out(&mut self, text: &str) {
        self.print_part(Part::Note, text);
    }

    pub fn dim(&self, s: &str) -> String {
        style::paint(self.display.color, style::DIM, s)
    }

    pub fn red(&self, s: &str) -> String {
        style::paint(self.display.color, style::RED, s)
    }

    pub fn say(&mut self, s: &str) {
        let t = self.dim(s);
        self.out(&t);
    }

    pub fn error(&mut self, s: &str) {
        let t = self.red(&format!("error: {s}"));
        self.out(&t);
    }

    pub fn notice(&mut self, s: impl Into<String>) {
        self.display.notice = Some((s.into(), Instant::now()));
        self.dirty = true;
    }

    /// Paths relative to the workspace and `~` for home: shorter, and what you would type yourself.
    pub(super) fn shorten(&self, s: &str) -> String {
        let cwd = self.app.cwd.display().to_string();
        let mut out = s.replace(&format!("{cwd}/"), "").replace(&cwd, ".");
        if let Some(h) = self
            .app
            .paths
            .home
            .as_ref()
            .map(|h| h.display().to_string())
            .filter(|h| h.len() > 1)
        {
            out = out.replace(&format!("{h}/"), "~/").replace(&h, "~");
        }
        out
    }

    /// `● Tool  what it is doing` on one row. The dot is the status: pulsing yellow while it runs
    /// (`ok` is `None`), solid green when it worked, solid red when it failed.
    pub(super) fn tool_row(
        &self,
        ok: Option<bool>,
        name: &str,
        desc: &str,
        running_for: Option<Duration>,
    ) -> String {
        let name = &tool_label(name);
        let dot_style = match ok {
            None => style::pulse(self.display.spinner),
            Some(true) => style::GREEN,
            Some(false) => style::RED,
        };
        let secs = running_for
            .filter(|d| d.as_secs() >= 3)
            .map(|d| format!(" {}s", d.as_secs()))
            .unwrap_or_default();
        let room = self
            .display
            .width
            .saturating_sub(4 + view::width_of(name) + view::width_of(&secs));
        let desc = view::truncate(&self.shorten(desc), room);
        if !self.display.color {
            return format!(
                "{} {name}  {desc}{secs}",
                if ok == Some(false) { "✗" } else { "●" }
            );
        }
        format!(
            "{} {}  {}{secs}{}",
            style::paint(true, dot_style, "●"),
            style::paint(true, style::BOLD, name),
            style::paint(true, style::DIM, &desc),
            style::RESET
        )
    }

    /// `label value` with the value in the colour that means something.
    pub fn kv(&self, label: &str, value: &str, style_: &str) -> String {
        format!(
            "{} {}",
            self.dim(label),
            style::paint(self.display.color, style_, value)
        )
    }

    pub(super) fn flush_partial(&mut self) {
        if !self.turn.partial.is_empty() {
            let line = std::mem::take(&mut self.turn.partial);
            let styled = self.display.styler.line(&line);
            self.print_part(Part::Reply, &styled);
        }
    }
}
