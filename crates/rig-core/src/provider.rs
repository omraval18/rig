//! The seam between the agent loop and model backends.

use crate::message::{Block, Message, Role};
use serde_json::Value;
use std::fmt;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema object.
    pub parameters: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effort {
    Low,
    Medium,
    High,
    XHigh,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "low" => Effort::Low,
            "medium" => Effort::Medium,
            "high" => Effort::High,
            "xhigh" | "max" => Effort::XHigh,
            _ => return None,
        })
    }
}

pub struct Request<'a> {
    pub system: &'a str,
    pub messages: &'a [Message],
    pub tools: &'a [ToolSpec],
    pub effort: Option<Effort>,
    /// Stable per-session id; used as the prompt-cache key where supported.
    pub session_id: &'a str,
    /// Checked while streaming; set it to abort the request.
    pub cancel: &'a AtomicBool,
}

pub enum StreamEvent<'a> {
    Text(&'a str),
    Reasoning(&'a str),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
}

impl std::ops::AddAssign for Usage {
    fn add_assign(&mut self, other: Usage) {
        self.input += other.input;
        self.output += other.output;
        self.cached += other.cached;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
}

pub struct Completion {
    pub message: Message,
    pub usage: Usage,
    pub stop: StopReason,
}

impl Completion {
    /// An assistant reply made of `blocks`. Tool calls take precedence over the stop reason:
    /// the turn is not over while a call is waiting to run.
    pub fn assistant(blocks: Vec<Block>, usage: Usage, hit_limit: bool) -> Self {
        let stop = if blocks.iter().any(|b| matches!(b, Block::ToolCall { .. })) {
            StopReason::ToolUse
        } else if hit_limit {
            StopReason::MaxTokens
        } else {
            StopReason::EndTurn
        };
        Self {
            message: Message {
                role: Role::Assistant,
                blocks,
            },
            usage,
            stop,
        }
    }
}

#[derive(Debug)]
pub enum ProviderError {
    /// Missing or rejected credentials. Never retried.
    Auth(String),
    Http {
        status: u16,
        body: String,
    },
    Network(String),
    /// The server answered, but not in the shape we understand.
    Protocol(String),
    /// The user cancelled the request.
    Cancelled,
}

impl ProviderError {
    pub fn is_retryable(&self) -> bool {
        match self {
            ProviderError::Network(_) => true,
            ProviderError::Http { status, .. } => *status == 429 || *status >= 500,
            _ => false,
        }
    }

    /// The request was too large for the model's context window.
    pub fn is_context_overflow(&self) -> bool {
        let text = match self {
            ProviderError::Http { status: 413, .. } => return true,
            ProviderError::Http { body, .. } => body.to_lowercase(),
            ProviderError::Protocol(m) => m.to_lowercase(),
            _ => return false,
        };
        [
            "context_length_exceeded",
            "maximum context length",
            "prompt is too long",
            "too many tokens",
            "context window",
            "input is too long",
            "exceeds the context",
            "request too large",
        ]
        .iter()
        .any(|p| text.contains(p))
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderError::Auth(m) => write!(f, "authentication: {m}"),
            ProviderError::Http { status, body } => {
                let body: String = body.chars().take(600).collect();
                write!(f, "HTTP {status}: {body}")
            }
            ProviderError::Network(m) => write!(f, "network: {m}"),
            ProviderError::Protocol(m) => write!(f, "protocol: {m}"),
            ProviderError::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// What a provider says about one of its models.
#[derive(Clone, Debug, Default)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub context_window: Option<u64>,
    pub efforts: Vec<Effort>,
    pub default_effort: Option<Effort>,
    pub hidden: bool,
}

pub trait Provider: Send + Sync {
    /// Provider id, also used to tag [`crate::message::Opaque`] replay state.
    fn name(&self) -> &str;
    fn model(&self) -> &str;
    /// Stream one assistant turn. `on_event` receives incremental text/reasoning.
    fn stream(
        &self,
        req: &Request,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<Completion, ProviderError>;

    /// Models this provider serves, when it can say. Used by the model picker.
    fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }
}
