//! Tool abstraction. Concrete tools live in `rig-tools`.

use crate::provider::ToolSpec;
use crate::skills::Skill;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    Low,
    Medium,
    High,
}

impl Risk {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "low" => Risk::Low,
            "medium" => Risk::Medium,
            "high" => Risk::High,
            _ => return None,
        })
    }
}

/// What a call intends to do. Drives the permission policy and scheduling.
#[derive(Clone, Debug)]
pub enum Access {
    /// Reads the local filesystem only.
    Read,
    /// Edits files.
    Write,
    /// Runs a shell command with the model-declared risk.
    Exec { risk: Risk, command: String },
    /// Reads from the network.
    Network,
}

impl Access {
    /// Safe to run next to other concurrency-safe calls.
    pub fn is_concurrent(&self) -> bool {
        matches!(self, Access::Read | Access::Network)
    }
}

#[derive(Clone, Debug)]
pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }

    pub fn err(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

/// Files as they were before one tool call changed them (`None` = did not exist).
#[derive(Clone, Debug, Default)]
pub struct UndoBatch {
    pub files: Vec<(PathBuf, Option<Vec<u8>>)>,
}

/// State shared by the agent loop, tools and front end.
#[derive(Default)]
pub struct Shared {
    /// Set by the front end to stop the current turn (streaming, tools, approvals).
    pub cancel: AtomicBool,
    pub undo: Mutex<Vec<UndoBatch>>,
    pub skills: RwLock<Vec<Skill>>,
    /// JSONL archive of compacted turns, searchable by the `recall` tool.
    pub archive: Mutex<Option<PathBuf>>,
}

#[derive(Clone)]
pub struct ToolCtx {
    pub cwd: PathBuf,
    pub session_id: String,
    pub shared: Arc<Shared>,
}

impl ToolCtx {
    pub fn new(cwd: PathBuf, session_id: String) -> Self {
        Self {
            cwd,
            session_id,
            shared: Arc::new(Shared::default()),
        }
    }

    pub fn cancelled(&self) -> bool {
        self.shared
            .cancel
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn access(&self, args: &Value) -> Access;
    /// One-line, human-readable description of this call for the UI and approvals.
    fn describe(&self, args: &Value) -> String;
    fn run(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput;
    /// What the user should see after the call finished: the line that matters from a command's
    /// output, a result count, the plan as a checklist. Multi-line is fine. `None` shows nothing for a success.
    fn summarize(&self, _args: &Value, _output: &ToolOutput) -> Option<String> {
        None
    }
}

#[derive(Default)]
pub struct Registry {
    tools: Vec<Box<dyn Tool>>,
}

impl Registry {
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        Self { tools }
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.spec().name == name)
            .map(Box::as_ref)
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|t| t.spec()).collect()
    }
}

/// The last line of `text` that says something: not blank and not a rule like `-----` or `=====`.
pub fn last_meaningful_line(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.chars().all(|c| "-=_~*#+ ".contains(c)))
        .map(|l| l.chars().take(160).collect())
}

/// For a failed command: the line that says what went wrong (`ls: x: No such file`, `FAILED ...`),
/// not merely the last one. Falls back to the last meaningful line.
pub fn error_line(text: &str) -> Option<String> {
    const SIGNS: [&str; 24] = [
        "error",
        "failed",
        "failure",
        "no such file",
        "not found",
        "denied",
        "cannot",
        "can't",
        "exception",
        "panicked",
        "could not",
        "couldn't",
        "unable to",
        "refused",
        "timed out",
        "timeout",
        "fatal",
        "invalid",
        "unreachable",
        "no route",
        "unknown host",
        "forbidden",
        "curl: (",
        "abort",
    ];
    text.lines()
        .rev()
        .map(str::trim)
        .find(|l| {
            !l.is_empty() && {
                let low = l.to_lowercase();
                SIGNS.iter().any(|s| low.contains(s))
            }
        })
        .map(|l| l.chars().take(160).collect())
        .or_else(|| last_meaningful_line(text))
}

#[cfg(test)]
mod tests;
