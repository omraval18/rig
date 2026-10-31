//! The main loop: terminal input, timers, and draining the agent worker's events.

use super::Tui;
use crossterm::event::{self, Event, KeyEventKind};
use std::time::{Duration, Instant};

impl Tui {
    pub(super) fn main_loop(&mut self) {
        while !self.quit {
            while let Ok(ev) = self.rx.try_recv() {
                self.on_ev(ev);
            }
            if (self.turn.running.is_some() || self.turn.compacting)
                && self.display.last_tick.elapsed() >= Duration::from_millis(100)
            {
                self.display.spinner += 1;
                self.display.last_tick = Instant::now();
                self.dirty = true;
            }
            if self.prompt.completion.as_ref().is_some_and(|c| c.loading) && self.index.ready() {
                self.refresh_completion();
            }
            // A background download can finish while rig is open: show it in the footer once it has.
            if self.app.staged_update.is_none()
                && self.update_poll.elapsed() >= Duration::from_secs(30)
            {
                self.update_poll = Instant::now();
                if let Some(v) = crate::update::staged_version(&self.app.paths.base) {
                    self.app.staged_update = Some(v);
                    self.dirty = true;
                }
            }
            if self
                .display
                .notice
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(4))
            {
                self.display.notice = None;
                self.dirty = true;
            }
            if self.dirty {
                self.draw();
                self.dirty = false;
            }
            match event::poll(Duration::from_millis(20)) {
                Ok(true) => match event::read() {
                    Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => self.on_key(k),
                    Ok(Event::Paste(s)) => {
                        if self.prompt.entry.is_some() {
                            self.paste_into_entry(&s);
                        } else if self.prompt.picker.is_none() && self.prompt.approval.is_none() {
                            self.paste_text(&s);
                        }
                        self.dirty = true;
                    }
                    Ok(Event::Resize(..)) => self.dirty = true,
                    Ok(Event::Mouse(m)) => {
                        let rows = self.display.height.saturating_sub(4);
                        match m.kind {
                            event::MouseEventKind::ScrollUp => {
                                self.display.transcript.scroll_by(3, rows)
                            }
                            event::MouseEventKind::ScrollDown => {
                                self.display.transcript.scroll_by(-3, rows)
                            }
                            _ => {}
                        }
                        self.dirty = true;
                    }
                    _ => {}
                },
                Ok(false) => {}
                Err(_) => break,
            }
        }
    }
}
