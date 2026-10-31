//! Runs the agent on a worker thread and reports to the UI over a channel.
//!
//! The UI thread owns the terminal and stays responsive; the worker owns the `Agent`
//! for the length of one job and hands it back in `Ev::Done`.

use rig_core::agent::{Agent, AgentError, Ui};
use rig_core::message::Message;
use rig_core::permission::Approval;
use rig_core::provider::{ModelInfo, ProviderError, Usage};
use rig_core::session::Session;
use rig_core::sync::MutexExt;
use rig_core::tool::{Shared, ToolOutput};
use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub enum Job {
    Turn(Message),
    Compact,
    /// `/summary`: read the session and show a summary; nothing is changed.
    Summary,
}

pub enum Ev {
    Text(String),
    Reasoning(String),
    ToolStart {
        name: String,
        desc: String,
    },
    ToolEnd {
        error: Option<String>,
        summary: Option<String>,
    },
    Usage(Usage),
    Context(Option<u8>),
    Retrying(String),
    Compacting(bool),
    Approval {
        name: String,
        desc: String,
        reason: String,
        remember: Option<String>,
        reply: Sender<Approval>,
    },
    /// The agent asks the person something; the answer (or `None`) goes back on `reply`.
    Question {
        question: String,
        options: Vec<String>,
        reply: Sender<Option<String>>,
    },
    Done {
        agent: Box<Agent>,
        result: Result<JobOutcome, AgentError>,
    },
    Models {
        provider: String,
        models: Result<Vec<ModelInfo>, String>,
    },
    Notice(String),
    /// The turn failed before the model answered; the prompt was taken back out of the history.
    RolledBack,
    LoginDone(Result<String, String>),
}

/// What a finished job did, beyond "no error".
#[derive(Default)]
pub struct JobOutcome {
    /// Set by `/compact`: how many messages and tool calls were archived.
    pub compacted: Option<(usize, usize, usize)>,
    pub nothing_to_compact: bool,
    /// Set by `/summary`.
    pub summary: Option<String>,
}

pub struct ChannelUi {
    pub tx: Sender<Ev>,
    pub queue: Arc<Mutex<VecDeque<Message>>>,
    pub session: Arc<Mutex<Session>>,
    pub shared: Arc<Shared>,
}

impl Ui for ChannelUi {
    fn text(&mut self, delta: &str) {
        let _ = self.tx.send(Ev::Text(delta.to_string()));
    }

    fn reasoning(&mut self, delta: &str) {
        let _ = self.tx.send(Ev::Reasoning(delta.to_string()));
    }

    fn tool_start(&mut self, name: &str, description: &str) {
        let _ = self.tx.send(Ev::ToolStart {
            name: name.into(),
            desc: description.into(),
        });
    }

    fn tool_end(&mut self, _name: &str, output: &ToolOutput, summary: Option<&str>) {
        let line = summary
            .map(String::from)
            .or_else(|| rig_core::tool::last_meaningful_line(&output.text));
        let error = output
            .is_error
            .then(|| line.clone().unwrap_or_else(|| "failed".into()));
        let _ = self.tx.send(Ev::ToolEnd {
            error,
            summary: summary.filter(|_| !output.is_error).map(String::from),
        });
    }

    fn usage(&mut self, u: &Usage) {
        let _ = self.tx.send(Ev::Usage(*u));
    }

    fn context(&mut self, percent: Option<u8>) {
        let _ = self.tx.send(Ev::Context(percent));
    }

    fn retrying(&mut self, attempt: u32, error: &ProviderError) {
        let _ = self
            .tx
            .send(Ev::Retrying(format!("retrying ({attempt}/3): {error}")));
    }

    fn compacting(&mut self, starting: bool) {
        let _ = self.tx.send(Ev::Compacting(starting));
    }

    fn message(&mut self, message: &Message) {
        let _ = self.session.locked().append(message);
    }

    fn checkpoint(&mut self, split: usize, checkpoint: &Message) {
        let _ = self.session.locked().append_checkpoint(split, checkpoint);
        let _ = self.tx.send(Ev::Notice(format!("Compacted context: {} older messages are now a checkpoint. Full text is archived; the agent can look it up with recall.", split.saturating_sub(1))));
    }

    fn rolled_back(&mut self, len: usize) {
        let _ = self.session.locked().append_truncate(len);
        let _ = self.tx.send(Ev::RolledBack);
    }

    fn queued(&mut self) -> Vec<Message> {
        self.queue.locked().drain(..).collect()
    }

    fn ask(&mut self, question: &str, options: &[String]) -> Option<String> {
        let (reply, answer) = channel();
        let ev = Ev::Question {
            question: question.into(),
            options: options.to_vec(),
            reply,
        };
        if self.tx.send(ev).is_err() {
            return None;
        }
        loop {
            if self.shared.cancel.load(Ordering::Relaxed) {
                return None;
            }
            match answer.recv_timeout(Duration::from_millis(100)) {
                Ok(a) => return a,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return None,
            }
        }
    }

    fn approve(
        &mut self,
        name: &str,
        description: &str,
        reason: &str,
        remember: Option<&str>,
    ) -> Approval {
        let (reply, answer) = channel();
        let ev = Ev::Approval {
            name: name.into(),
            desc: description.into(),
            reason: reason.into(),
            remember: remember.map(String::from),
            reply,
        };
        if self.tx.send(ev).is_err() {
            return Approval::No;
        }
        loop {
            if self.shared.cancel.load(Ordering::Relaxed) {
                return Approval::No;
            }
            match answer.recv_timeout(Duration::from_millis(100)) {
                Ok(a) => return a,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Approval::No,
            }
        }
    }
}

/// Start `job` on a worker thread; the agent comes back through `Ev::Done`.
pub fn spawn(mut agent: Box<Agent>, job: Job, mut ui: ChannelUi) {
    std::thread::spawn(move || {
        let result = match job {
            Job::Turn(msg) => agent.run_turn(msg, &mut ui).map(|()| JobOutcome::default()),
            Job::Summary => agent
                .summarize()
                .map(|text| JobOutcome {
                    summary: Some(text),
                    ..JobOutcome::default()
                })
                .map_err(Into::into),
            Job::Compact => match agent.compact(&mut ui) {
                Ok(Some(cp)) => Ok(JobOutcome {
                    compacted: Some((
                        cp.archived_messages,
                        cp.archived_tool_calls,
                        cp.unverified_handles,
                    )),
                    ..JobOutcome::default()
                }),
                Ok(None) => Ok(JobOutcome {
                    nothing_to_compact: true,
                    ..JobOutcome::default()
                }),
                Err(e) => Err(e.into()),
            },
        };
        let _ = ui.tx.send(Ev::Done { agent, result });
    });
}
