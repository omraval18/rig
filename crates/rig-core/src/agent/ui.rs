//! What the agent needs from a front end, and how a turn can fail.

use crate::message::Message;
use crate::permission::Approval;
use crate::provider::{ProviderError, Usage};
use crate::tool::ToolOutput;
use std::fmt;

/// Everything the agent needs from the front end.
pub trait Ui {
    fn text(&mut self, _delta: &str) {}
    fn reasoning(&mut self, _delta: &str) {}
    fn tool_start(&mut self, _name: &str, _description: &str) {}
    fn tool_end(&mut self, _name: &str, _output: &ToolOutput, _summary: Option<&str>) {}
    fn usage(&mut self, _usage: &Usage) {}
    fn retrying(&mut self, _attempt: u32, _error: &ProviderError) {}
    /// Called for every message appended to the history, for persistence.
    fn message(&mut self, _message: &Message) {}
    /// History `1..split` was replaced by `checkpoint`; persist that.
    fn checkpoint(&mut self, _split: usize, _checkpoint: &Message) {}
    fn compacting(&mut self, _starting: bool) {}
    /// The history was cut back to `len` messages because the turn failed before the model answered.
    fn rolled_back(&mut self, _len: usize) {}
    /// Percent of the context window the next request will use, when the window is known.
    fn context(&mut self, _percent: Option<u8>) {}
    /// Messages the user typed while the turn was running, delivered at the next model step.
    fn queued(&mut self) -> Vec<Message> {
        Vec::new()
    }
    /// Ask the user a question with options; `None` means no answer (non-interactive, or cancelled).
    /// The user can always type an answer of their own.
    fn ask(&mut self, _question: &str, _options: &[String]) -> Option<String> {
        None
    }
    /// Ask the user to approve a gated call. Non-interactive front ends answer `No`.
    /// `remember` names the rule an "always" answer would store, when one is possible.
    fn approve(
        &mut self,
        _name: &str,
        _description: &str,
        _reason: &str,
        _remember: Option<&str>,
    ) -> Approval {
        Approval::No
    }
}

#[derive(Debug)]
pub enum AgentError {
    Provider(ProviderError),
    TurnLimit(usize),
    Cancelled,
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentError::Provider(e) => write!(f, "{e}"),
            AgentError::TurnLimit(n) => {
                write!(f, "stopped after {n} model turns without finishing")
            }
            AgentError::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<ProviderError> for AgentError {
    fn from(e: ProviderError) -> Self {
        if matches!(e, ProviderError::Cancelled) {
            AgentError::Cancelled
        } else {
            AgentError::Provider(e)
        }
    }
}
