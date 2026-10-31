//! `/new`, `/resume`, `/fork`, `/rename`, `/compact`: sessions and their lifecycle.

use crate::picker::{PickItem, PickKind};
use crate::runtime::Job;
use crate::tui::{Tui, is_hidden, visible_text};
use rig_core::message::{Block, Role};
use rig_core::session::{Session, age, archive_path};
use rig_core::sync::MutexExt;
use std::path::PathBuf;

use super::{BUSY, first_line};

impl Tui {
    pub(crate) fn idle_agent(&mut self) -> bool {
        if self.agent.is_none() {
            self.say(BUSY);
            return false;
        }
        true
    }

    pub(super) fn adopt_session(
        &mut self,
        session: Session,
        history: Vec<rig_core::message::Message>,
    ) {
        let Some(agent) = self.agent.as_mut() else {
            return;
        };
        agent.history = history;
        agent.ctx.session_id = session.id.clone();
        *self.shared.archive.locked() = Some(archive_path(&session.path));
        self.shared.undo.locked().clear();
        agent.reset_measure();
        self.info.ctx_pct = agent.context_percent();
        *self.session.locked() = session;
    }

    pub(super) fn cmd_new(&mut self) {
        if !self.idle_agent() {
            return;
        }
        let label = format!("{}/{}", self.info.spec.name, self.info.model);
        // Leaving a session nobody typed in: do not keep an empty file around.
        let abandoned = self.agent.as_ref().is_some_and(|a| {
            !a.history.iter().any(|m| {
                m.role == rig_core::message::Role::User
                    && !crate::tui::is_hidden(m)
                    && !rig_core::compact::is_checkpoint(m)
            })
        });
        let old_path = self.session.locked().path.clone();
        match Session::create(&self.app.paths.root, &self.app.cwd, &label) {
            Ok(mut s) => {
                if abandoned {
                    let _ = std::fs::remove_file(archive_path(&old_path));
                    let _ = std::fs::remove_file(&old_path);
                }
                let reminder = self.app.reminder(&self.skills);
                let _ = s.append(&reminder);
                self.adopt_session(s, vec![reminder]);
                self.stats = Default::default();
                self.turn.last_reply.clear();
                self.index.refresh();
                self.record_session_settings();
                self.reset_screen();
            }
            Err(e) => self.error(&format!("cannot create a session: {e}")),
        }
    }

    pub(super) fn cmd_resume(&mut self) {
        if !self.idle_agent() {
            return;
        }
        let current = self.session.locked().id.clone();
        let items: Vec<PickItem> = Session::list(&self.app.paths.root, &self.app.cwd)
            .into_iter()
            .map(|s| {
                PickItem::new(first_line(&s.title, 70), s.path.display().to_string())
                    .detail(format!("{} · {} msgs", age(s.modified), s.messages))
                    .current(s.id == current)
            })
            .collect();
        if items.is_empty() {
            self.say("No saved sessions in this folder yet.");
            return;
        }
        self.open_picker(PickKind::Resume, "Resume a session", items);
    }

    pub(super) fn load_session(&mut self, path: PathBuf) {
        if !self.idle_agent() {
            return;
        }
        let history = match Session::load(&path) {
            Ok(h) if !h.is_empty() => h,
            Ok(_) => return self.error("that session is empty"),
            Err(e) => return self.error(&format!("cannot read the session: {e}")),
        };
        match Session::reopen(&path) {
            Ok(s) => {
                self.adopt_session(s, history.clone());
                self.restore_session_settings(&path);
                self.reset_screen();
                self.recap(&history);
            }
            Err(e) => self.error(&e.to_string()),
        }
    }

    pub(super) fn cmd_rename(&mut self, title: &str) {
        if title.is_empty() {
            return self.say("Usage: /rename <title>");
        }
        let result = self.session.locked().set_title(title);
        match result {
            Ok(()) => self.say(&format!("Session renamed to \"{title}\".")),
            Err(e) => self.error(&e.to_string()),
        }
    }

    pub(super) fn cmd_fork(&mut self) {
        let Some(agent) = self.agent.as_ref() else {
            return self.say(BUSY);
        };
        let items: Vec<PickItem> = agent
            .history
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                m.role == Role::User
                    && !is_hidden(m)
                    && m.blocks.iter().any(|b| matches!(b, Block::Text { .. }))
            })
            .map(|(i, m)| {
                PickItem::new(first_line(&visible_text(m), 80), i.to_string())
                    .detail(format!("message {i}"))
            })
            .collect();
        if items.is_empty() {
            return self.say("Nothing to fork from yet.");
        }
        self.open_picker(
            PickKind::Fork,
            "Fork before which prompt? (the new session starts just before it)",
            items,
        );
    }

    pub(super) fn fork_at(&mut self, idx: usize) {
        let Some(agent) = self.agent.as_ref() else {
            return self.say(BUSY);
        };
        let Some(msg) = agent.history.get(idx) else {
            return;
        };
        let prompt = msg
            .blocks
            .iter()
            .find_map(|b| {
                if let Block::Text { text } = b {
                    Some(text.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let before = agent.history[..idx].to_vec();
        let from = self.session.locked().path.clone();
        let label = format!("{}/{}", self.info.spec.name, self.info.model);
        match Session::fork(&self.app.paths.root, &self.app.cwd, &label, &from, &before) {
            Ok(s) => {
                self.adopt_session(s, before.clone());
                self.record_session_settings();
                self.reset_screen();
                self.recap(&before);
                self.prompt.composer.set_text(&prompt);
                self.say("Forked. The prompt is back in the editor so you can change it.");
            }
            Err(e) => self.error(&format!("cannot fork: {e}")),
        }
    }

    pub(super) fn cmd_compact(&mut self) {
        if !self.idle_agent() {
            return;
        }
        self.turn.compacting = true;
        self.launch(Job::Compact);
    }

    /// Summarize the session in the background; the summary is shown when it is ready.
    pub(super) fn cmd_summary(&mut self) {
        if !self.idle_agent() {
            return;
        }
        self.launch(Job::Summary);
    }

    pub(super) fn cmd_undo(&mut self) {
        match rig_tools::undo_last(&self.shared) {
            Some(m) => self.say(&m),
            None => self.say("Nothing to undo."),
        }
    }
}
