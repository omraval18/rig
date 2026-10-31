//! Starting a turn and settling it when the worker hands the agent back.

use super::Tui;
use super::format::{explain, fmt_duration, fmt_tokens};
use crate::runtime::{self, ChannelUi, Job, JobOutcome};
use rig_core::agent::{Agent, AgentError};
use rig_core::message::{Message, Role};
use rig_core::provider::Usage;
use rig_core::sync::MutexExt;
use std::time::Instant;

impl Tui {
    pub(super) fn finish_job(
        &mut self,
        mut agent: Box<Agent>,
        result: Result<JobOutcome, AgentError>,
    ) {
        self.flush_partial();
        self.turn.tools.clear();
        self.turn.compacting = false;
        std::mem::take(&mut self.pending).apply(&mut agent);
        self.info.ctx_pct = agent.context_percent();
        self.agent = Some(agent);
        let elapsed = self.turn.running.take().map(|t| t.elapsed());
        self.prompt.approval = None;
        match &result {
            Ok(o) => {
                if let Some((m, t, bad)) = o.compacted {
                    let extra = if bad > 0 {
                        format!(
                            "; {bad} citation(s) the archive could not confirm were marked unverified"
                        )
                    } else {
                        String::new()
                    };
                    self.say(&format!("Archived {m} messages and {t} tool calls{extra}."));
                }
                if o.nothing_to_compact {
                    self.say("Nothing to compact yet.");
                }
            }
            Err(AgentError::Cancelled) => self.say("Interrupted."),
            Err(e) => {
                let (msg, retry) = explain(e, &self.info.spec.name);
                self.error(&msg);
                if retry
                    && std::mem::take(&mut self.turn.rolled_back)
                    && self.prompt.composer.is_empty()
                    && !self.turn.last_prompt.is_empty()
                {
                    let prompt = std::mem::take(&mut self.turn.last_prompt);
                    self.prompt.composer.insert_str(&prompt);
                    self.notice("your message is back in the prompt: press Enter to retry");
                }
            }
        }
        self.turn.rolled_back = false;
        let u = self.turn.usage;
        if u.output > 0 {
            self.stats.turns += 1;
            self.stats.total += u;
            let cached = if u.cached > 0 {
                format!(" · {} cached", fmt_tokens(u.cached))
            } else {
                String::new()
            };
            let took = elapsed
                .filter(|e| e.as_secs() >= 2)
                .map(|e| format!(" · {}", fmt_duration(e)))
                .unwrap_or_default();
            self.say(&format!(
                "[{} in · {} out{cached}{took}]",
                fmt_tokens(u.input),
                fmt_tokens(u.output)
            ));
        }
        self.turn.usage = Usage::default();
        self.turn.last_reply = std::mem::take(&mut self.turn.reply);
        let rules = self.policy.allowlist();
        if rules != self.saved_allowlist {
            self.app.save_allowlist(&rules);
            self.saved_allowlist = rules;
        }

        let queued: Vec<Message> = self.steer.locked().drain(..).collect();
        if matches!(result, Err(AgentError::Cancelled)) {
            if !queued.is_empty() {
                self.say(&format!("Dropped {} queued message(s).", queued.len()));
            }
        } else if !queued.is_empty() {
            // The turn ended before the model took another step; run the steering as the next turn.
            let blocks = queued.into_iter().flat_map(|m| m.blocks).collect();
            self.launch(Job::Turn(Message {
                role: Role::User,
                blocks,
            }));
        }
        self.dirty = true;
    }

    pub fn channel_ui(&self) -> ChannelUi {
        ChannelUi {
            tx: self.tx.clone(),
            steer: self.steer.clone(),
            session: self.session.clone(),
            shared: self.shared.clone(),
        }
    }

    pub fn launch(&mut self, job: Job) {
        let Some(agent) = self.agent.take() else {
            return;
        };
        self.shared
            .cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.turn.running = Some(Instant::now());
        self.turn.reply.clear();
        self.turn.usage = Usage::default();
        runtime::spawn(agent, job, self.channel_ui());
        self.dirty = true;
    }
}
