//! Turning the prompt into a message and starting a turn (or steering the running one).

use super::{Part, Tui};
use crate::runtime::Job;
use rig_core::expand;
use rig_core::message::{Block, Message, Role};
use rig_core::sync::MutexExt;
use rig_core::text::count;

impl Tui {
    /// The user's text plus context for its `@` and `$` tokens.
    pub(super) fn compose(&mut self, text: &str) -> Message {
        let ex = expand::expand(
            text,
            &self.app.cwd,
            self.app.paths.home.as_deref(),
            &self.skills,
        );
        let mut blocks = vec![Block::Text {
            text: text.to_string(),
        }];
        blocks.extend(ex.reminders.iter().map(|r| Block::Text { text: r.clone() }));
        let pasted = self.take_images(text);
        if !pasted.is_empty() {
            let line = self.dim(&format!("+ {}", count(pasted.len(), "image")));
            self.print_part(Part::Tool, &line);
        }
        blocks.extend(pasted);
        blocks.extend(ex.images.iter().cloned());
        if !ex.is_empty() {
            let t = self.dim(&format!("+ {}", ex.summary()));
            self.print_part(Part::Tool, &t);
        }
        for m in &ex.missing {
            let t = self.dim(&format!("! {m} not found; sent as plain text"));
            self.print_part(Part::Tool, &t);
        }
        Message {
            role: Role::User,
            blocks,
        }
    }

    /// Your message as a shaded, full-width card so it stands apart from the agent's output.
    pub(super) fn echo_user(&mut self, text: &str) {
        self.begin_part(Part::User);
        self.display.transcript.push_card(text.trim_end());
        // Sending a message jumps back to the live end of the conversation.
        self.display.transcript.scroll = 0;
        self.dirty = true;
    }

    pub fn submit(&mut self) {
        let text = self.prompt.composer.submit();
        self.prompt.completion = None;
        if text.trim().is_empty() {
            self.dirty = true;
            return;
        }
        self.app.append_history(&text);
        if text.starts_with('/') {
            self.echo_user(&text);
            self.run_slash(&text);
            self.dirty = true;
            return;
        }
        if !self.signed_in() {
            // Keep what they typed; they will want to send it once signed in.
            self.prompt.composer.insert_str(&text);
            let hint = crate::app::sign_in_hint(&self.info.spec);
            self.notice(hint);
            return;
        }
        if self.turn.running.is_some() || self.turn.compacting {
            self.echo_user(&text);
            let msg = self.compose(&text);
            self.steer.locked().push_back(msg);
            self.say("  ↳ queued; the agent will see it at its next step");
        } else if self.agent.is_some() {
            self.echo_user(&text);
            let msg = self.compose(&text);
            self.turn.last_prompt = text;
            self.turn.rolled_back = false;
            self.launch(Job::Turn(msg));
        }
        self.dirty = true;
    }
}
