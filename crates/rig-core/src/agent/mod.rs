//! The agent loop: stream a turn, run the requested tools, feed results back.
//!
//! Behaviors that matter:
//! - tool calls from one assistant turn run in order, but consecutive
//!   read-only calls run concurrently; an edit or command always runs alone;
//! - a denied call is returned to the model as an error result, never silently dropped;
//! - transient provider errors are retried only if nothing has been shown yet;
//! - the user can cancel at any moment and queue between model steps;
//! - when the window fills, or the provider reports an overflow, older context is compacted.

use crate::compact;
use crate::message::Message;
use crate::permission::Policy;
use crate::provider::{
    Completion, Effort, Provider, ProviderError, Request, StreamEvent, ToolSpec,
};
use crate::sync::MutexExt;
use crate::tool::{Registry, ToolCtx};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

mod calls;
mod ui;

pub use ui::{AgentError, Ui};

pub struct Agent {
    pub provider: Box<dyn Provider>,
    pub tools: Registry,
    pub policy: Policy,
    pub system: String,
    pub effort: Option<Effort>,
    pub ctx: ToolCtx,
    pub history: Vec<Message>,
    pub max_turns: usize,
    /// Model context window in tokens (0 = unknown, disables automatic compaction).
    pub context_window: u64,
    /// Compact when the request reaches this percent of the window.
    pub auto_compact_percent: u8,
    /// Input tokens the provider reported for the latest request, and the history length it covered.
    last_input: u64,
    last_len: usize,
    /// After a compaction, don't compact again until the context has grown past this many tokens.
    compact_floor: u64,
}

impl Agent {
    pub fn new(
        provider: Box<dyn Provider>,
        tools: Registry,
        policy: Policy,
        system: String,
        ctx: ToolCtx,
        history: Vec<Message>,
    ) -> Self {
        Self {
            provider,
            tools,
            policy,
            system,
            effort: Some(Effort::Medium),
            ctx,
            history,
            max_turns: 100,
            context_window: 0,
            auto_compact_percent: 80,
            last_input: 0,
            last_len: 0,
            compact_floor: 0,
        }
    }

    /// Tokens the next request will roughly carry: what the provider last measured plus what was added since.
    pub fn context_tokens(&self) -> u64 {
        if self.last_input > 0 && self.last_len <= self.history.len() {
            self.last_input + compact::estimate_tokens(&self.history[self.last_len..]) as u64
        } else {
            compact::estimate_tokens(&self.history) as u64 + 2_000
        }
    }

    /// Forget the provider's token measurement after the history was replaced (new or resumed session).
    pub fn reset_measure(&mut self) {
        self.last_input = 0;
        self.last_len = 0;
        self.compact_floor = 0;
    }

    pub fn context_percent(&self) -> Option<u8> {
        (self.context_window > 0)
            .then(|| ((self.context_tokens() * 100) / self.context_window).min(999) as u8)
    }

    /// Convenience for scripts: one plain-text prompt.
    pub fn run_text(&mut self, input: &str, ui: &mut dyn Ui) -> Result<(), AgentError> {
        self.run_turn(Message::user(input), ui)
    }

    pub fn run_turn(&mut self, user: Message, ui: &mut dyn Ui) -> Result<(), AgentError> {
        self.ctx.shared.cancel.store(false, Ordering::Relaxed);
        // A tool call left unanswered by an interrupted turn would make the provider reject everything after it.
        crate::message::repair_dangling_calls(&mut self.history);
        let start = self.history.len();
        self.push(user, ui);
        let specs = self.tools.specs();
        let mut recovered = false;

        for step in 0..self.max_turns {
            if self.cancelled() {
                return Err(AgentError::Cancelled);
            }
            for queue in ui.queued() {
                self.push(queue, ui);
            }
            if self.should_compact() {
                // A failed automatic compaction must not end the turn; the overflow path retries it.
                let _ = self.compact(ui);
            }

            let completion = match self.sample(&specs, ui) {
                Ok(c) => c,
                Err(AgentError::Provider(e)) if e.is_context_overflow() && !recovered => {
                    recovered = true;
                    match self.compact(ui) {
                        Ok(Some(_)) => continue,
                        _ => return Err(AgentError::Provider(e)),
                    }
                }
                Err(e) => {
                    // Nothing came back for this prompt: take it out of the history so a retry does not send it twice.
                    if step == 0
                        && matches!(e, AgentError::Provider(ref p) if !matches!(p, ProviderError::Cancelled))
                    {
                        self.history.truncate(start);
                        ui.rolled_back(start);
                    }
                    return Err(e);
                }
            };
            ui.usage(&completion.usage);
            if completion.usage.input > 0 {
                self.last_input = completion.usage.input + completion.usage.output;
                self.last_len = self.history.len() + 1;
            }
            ui.context(self.context_percent());
            let calls: Vec<(String, String, serde_json::Value)> = completion
                .message
                .tool_calls()
                .map(|(i, n, a)| (i.to_string(), n.to_string(), a.clone()))
                .collect();
            self.push(completion.message, ui);

            if calls.is_empty() {
                return Ok(());
            }
            let results = self.run_calls(calls, ui);
            self.push(Message::tool_results(results), ui);
        }
        Err(AgentError::TurnLimit(self.max_turns))
    }

    fn cancelled(&self) -> bool {
        self.ctx.cancelled()
    }

    fn push(&mut self, m: Message, ui: &mut dyn Ui) {
        ui.message(&m);
        self.history.push(m);
    }

    fn should_compact(&self) -> bool {
        let tokens = self.context_tokens();
        self.context_window > 0
            && tokens * 100 >= self.context_window * self.auto_compact_percent as u64
            && tokens >= self.compact_floor
    }

    /// Compact now. `Ok(None)` when there is nothing worth compacting.
    /// A short summary of the session so far, for `/summary`. The history is not changed.
    pub fn summarize(&self) -> Result<String, ProviderError> {
        compact::summary::summarize(
            self.provider.as_ref(),
            &self.history,
            &self.ctx.session_id,
            &self.ctx.shared.cancel,
        )
    }

    pub fn compact(
        &mut self,
        ui: &mut dyn Ui,
    ) -> Result<Option<compact::Checkpoint>, ProviderError> {
        let archive = self.ctx.shared.archive.locked().clone();
        let Some(archive) = archive else {
            return Ok(None);
        };
        ui.compacting(true);
        // Keep roughly a fifth of the window verbatim.
        let tail = if self.context_window > 0 {
            (self.context_window / 5) as usize
        } else {
            20_000
        };
        let result = compact::compact(
            self.provider.as_ref(),
            &mut self.history,
            &archive,
            tail,
            &self.ctx.session_id,
            &self.ctx.shared.cancel,
        );
        ui.compacting(false);
        if let Ok(Some(cp)) = &result {
            ui.checkpoint(cp.split, &cp.message);
            self.last_input = 0;
            // Require a tenth of the window of new material before compacting again, so a window
            // that stays near its limit does not summarize on every step.
            self.compact_floor = self.context_tokens() + self.context_window / 10;
        }
        result
    }

    fn sample(&self, specs: &[ToolSpec], ui: &mut dyn Ui) -> Result<Completion, AgentError> {
        let req = Request {
            system: &self.system,
            messages: &self.history,
            tools: specs,
            effort: self.effort,
            session_id: &self.ctx.session_id,
            cancel: &self.ctx.shared.cancel,
        };
        let mut attempt = 0;
        loop {
            let mut emitted = false;
            let result = self.provider.stream(&req, &mut |ev| {
                emitted = true;
                match ev {
                    StreamEvent::Text(t) => ui.text(t),
                    StreamEvent::Reasoning(t) => ui.reasoning(t),
                }
            });
            match result {
                Ok(c) => return Ok(c),
                Err(e) if e.is_retryable() && !emitted && attempt < 3 && !self.cancelled() => {
                    attempt += 1;
                    ui.retrying(attempt, &e);
                    thread::sleep(Duration::from_secs(1 << attempt));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests;
