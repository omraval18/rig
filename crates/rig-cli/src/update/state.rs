//! What the updater remembers between runs, in `<rig home>/update/state.json`. It is shared by all
//! profiles, because there is one binary.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// Unix seconds of the last check, successful or not. Keeps a flaky network from being retried
    /// on every start.
    #[serde(default)]
    pub last_checked: u64,
    /// Newest release the last check saw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    /// A verified release waiting to be swapped in on the next start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staged: Option<String>,
    /// Set by `rig update --off`: no automatic checks or downloads.
    #[serde(default)]
    pub disabled: bool,
}

impl State {
    /// A missing or unreadable file is the default state, never an error.
    pub fn load(path: &Path) -> State {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        rig_core::fsutil::write_atomic(path, text.as_bytes(), 0o600).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_and_a_missing_file_is_the_default() {
        let dir = std::env::temp_dir().join(format!("rig-update-state-{}", std::process::id()));
        let path = dir.join("state.json");
        assert_eq!(State::load(&path), State::default());
        let s = State {
            last_checked: 42,
            latest: Some("0.2.0".into()),
            staged: Some("0.2.0".into()),
            disabled: true,
        };
        s.save(&path).unwrap();
        assert_eq!(State::load(&path), s);
        std::fs::remove_dir_all(dir).ok();
    }
}
