//! What you chose last time, remembered between sessions in `~/.rig/state.json`.
//!
//! `config.toml` is yours to edit; this file is rig's to write. A value here beats the config file
//! (it is what you picked most recently) and loses to a command-line flag.

use serde::{Deserialize, Serialize};
use std::path::Path;

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct State {
    /// `provider/model` of the last model you switched to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `low`..`xhigh`, or `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Permission level you last set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autonomy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_reasoning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_percent: Option<u8>,
    /// Start new chats with the model, effort and permissions you used last. On by default.
    #[serde(default = "yes")]
    pub remember: bool,
    /// Mouse wheel scrolls the conversation (hold Shift/Option to select text). On by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouse: Option<bool>,
}

impl Default for State {
    fn default() -> Self {
        State {
            model: None,
            effort: None,
            autonomy: None,
            show_reasoning: None,
            auto_compact_percent: None,
            remember: true,
            mouse: None,
        }
    }
}

impl State {
    pub fn load(path: &Path) -> State {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Write atomically so a crash never leaves half a file; an unchanged state is not rewritten.
    pub fn save(&self, path: &Path) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = rig_core::fsutil::write_atomic(path, text.as_bytes(), 0o644);
        }
    }

    /// The saved choice, or `None` when remembering is switched off.
    pub fn last<'a>(&'a self, v: &'a Option<String>) -> Option<&'a str> {
        v.as_deref().filter(|_| self.remember)
    }
}

#[cfg(test)]
mod tests;
