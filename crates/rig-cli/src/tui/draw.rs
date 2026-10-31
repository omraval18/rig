//! Composing the frame: footer, activity line, and the window layout.

use super::Tui;
use crate::style;
use crate::view::{self, Seg, View};
use crossterm::terminal;
use rig_core::sync::MutexExt;
use std::time::Duration;

impl Tui {
    /// Labels are dim; what they label is coloured: model cyan, effort violet, access by how much it allows.
    pub(super) fn footer(&self) -> (Vec<Seg>, Vec<Seg>) {
        let access = self.policy.autonomy();
        let mut left: Vec<Seg> = Vec::new();
        if !self.app.paths.is_default_profile() {
            left.push((self.app.paths.profile.clone(), style::BOLD));
            left.push((" · ".into(), ""));
        }
        left.extend([
            (format!("{}/", self.info.spec.name), ""),
            (self.info.model.clone(), style::CYAN),
            (" · effort ".into(), ""),
            self.info.effort.map_or(("off".into(), ""), |e| {
                (e.as_str().to_string(), style::VIOLET)
            }),
            (" · permissions ".into(), ""),
            (access.as_str().to_string(), style::permission(access)),
        ]);
        if !self.signed_in() {
            left.push((" · not signed in".into(), style::YELLOW));
        }
        if let Some(g) = &self.goal {
            left.push((
                format!(" · goal {}/{}", g.rounds, super::goal::MAX_ROUNDS),
                style::CYAN,
            ));
        }
        if let Some(v) = self.app.staged_update {
            left.push((format!(" · rig {v} ready, restart to update"), style::GREEN));
        }
        let right = match self.info.ctx_pct {
            Some(p) => vec![(format!("{p}% ctx"), style::context(p))],
            None if self.info.context_window > 0 => vec![("0% ctx".to_string(), "")],
            None => vec![],
        };
        (left, right)
    }

    pub fn draw(&mut self) {
        let activity = if self.turn.compacting {
            Some(format!(
                "{} Compacting context…",
                SPIN[self.display.spinner % SPIN.len()]
            ))
        } else {
            self.turn.running.map(|t| {
                let queued = self.queue.locked().len();
                let q = if queued > 0 {
                    format!(" · {queued} queued")
                } else {
                    String::new()
                };
                format!(
                    "{} Working {}s · Esc to interrupt · Enter to queue{q}",
                    SPIN[self.display.spinner % SPIN.len()],
                    t.elapsed().as_secs()
                )
            })
        };
        if let Some((_, at)) = &self.display.notice
            && at.elapsed() > Duration::from_secs(4)
        {
            self.display.notice = None;
        }
        let (fl, fr) = self.footer();
        let skills = &self.skills;
        let is_skill = |n: &str| skills.iter().any(|s| s.name == n);
        let approval_view = self.prompt.approval.as_ref().map(|a| &a.view);
        let tool_rows: Vec<String> = self
            .turn
            .tools
            .iter()
            .map(|t| self.tool_row(None, &t.name, &t.desc, Some(t.at.elapsed())))
            .collect();
        let frame = view::render(&View {
            width: self.display.width,
            tool_rows,
            color: self.display.color,
            composer: &self.prompt.composer,
            completion: self.prompt.completion.as_ref(),
            picker: self.prompt.picker.as_ref(),
            entry: self.prompt.entry.as_ref(),
            question: self.prompt.question.as_ref().map(|p| &p.view),
            approval: approval_view,
            partial: &self.turn.partial,
            activity,
            notice: self.display.notice.as_ref().map(|(s, _)| s.clone()),
            footer_left: fl,
            footer_right: fr,
            is_skill: &is_skill,
        });
        self.paint(frame);
    }

    /// Lay out the window: the conversation fills what the prompt area leaves, the prompt sits at the bottom.
    pub(super) fn paint(&mut self, frame: view::Frame) {
        let (w, h) = terminal::size()
            .map_or((self.display.width, self.display.height), |(w, h)| {
                (w as usize, h as usize)
            });
        if (w, h) != (self.display.width, self.display.height) {
            self.display.width = w;
            self.display.height = h;
            self.display.transcript.set_width(w);
            let _ = self.display.screen.invalidate();
        }
        let mut lines = frame.lines;
        let mut cursor_row = frame.cursor.0;
        // A prompt area taller than the window shows its bottom part.
        if lines.len() > h {
            let cut = lines.len() - h;
            lines.drain(..cut);
            cursor_row = cursor_row.saturating_sub(cut);
        }
        let viewport = h - lines.len();
        let mut rows = self.display.transcript.view(viewport);
        if self.display.transcript.scroll > 0 && viewport > 0 {
            let note = format!(
                "↓ {} more rows below · End jumps to the latest",
                self.display.transcript.scroll
            );
            rows[viewport - 1] =
                style::paint(self.display.color, style::DIM, &format!("{note:>w$}"));
        }
        rows.extend(lines);
        let _ = self
            .display
            .screen
            .paint(&rows, w, Some((viewport + cursor_row, frame.cursor.1)));
    }
}

const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
