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
    /// `provider/model` of models switched to, newest first, for the top of the model picker.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_models: Vec<String>,
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
    /// Mouse interaction for scrolling and transcript selection. On by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouse: Option<bool>,
}

impl Default for State {
    fn default() -> Self {
        State {
            model: None,
            recent_models: Vec::new(),
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
    /// Most models kept in `recent_models`.
    const RECENT: usize = 5;

    /// Put `model` at the front of the recent list.
    pub fn touch_model(&mut self, model: &str) {
        self.recent_models.retain(|m| m != model);
        self.recent_models.insert(0, model.to_string());
        self.recent_models.truncate(Self::RECENT);
    }

    pub fn mouse_enabled(&self) -> bool {
        self.mouse.unwrap_or(true)
    }

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
