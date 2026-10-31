//! Turns config + flags into a ready-to-run [`Agent`].

mod limits;
mod provider;
mod start;

use crate::config::{Config, Paths};
use rig_core::message::Message;
use rig_core::prompt::{ContextInput, session_context};
use rig_core::skills::{self, Skill};
use std::path::PathBuf;

pub use provider::{Credential, sign_in_hint};
pub use start::Started;

/// Prompts kept for Up-arrow recall.
const HISTORY_KEEP: usize = 500;

#[derive(Default, Clone)]
pub struct RunOpts {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub auto: Option<String>,
    pub cont: bool,
    pub resume: Option<String>,
    /// Sent as the first message when the interactive UI opens (`rig "fix the tests"`).
    pub first_message: Option<String>,
}

pub struct App {
    pub paths: Paths,
    pub cfg: Config,
    /// API keys saved in this profile.
    pub keys: crate::keys::Keys,
    pub cwd: PathBuf,
    /// What you picked last time (see `state.rs`).
    pub state: crate::state::State,
    /// A verified update waiting for the next start, read once at startup for the footer.
    pub staged_update: Option<crate::update::release::Version>,
    /// The MCP servers from the config. Nothing is started until the model uses one.
    pub mcp: std::sync::Arc<rig_mcp::Hub>,
}

impl App {
    /// `profile` is the `--profile` flag; see [`Paths::discover`] for the rest of the order.
    pub fn load(profile: Option<&str>) -> Result<App, String> {
        let cwd = std::env::current_dir()
            .map_err(|e| format!("cannot read the working directory: {e}"))?;
        let paths = Paths::discover(profile)?;
        let cfg = Config::load(&paths.config_files(&cwd))?;
        let state = crate::state::State::load(&paths.state_file());
        let keys = crate::keys::Keys::load(&paths.keys_file());
        let staged_update = crate::update::staged_version(&paths.base);
        let mcp = std::sync::Arc::new(rig_mcp::Hub::new(&cfg.mcp));
        Ok(App {
            paths,
            cfg,
            keys,
            cwd,
            state,
            staged_update,
            mcp,
        })
    }

    pub fn getenv(name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    pub fn save_state(&self) {
        self.state.save(&self.paths.state_file());
    }

    /// Previously approved "always allow" rules.
    pub fn load_allowlist(&self) -> Vec<String> {
        std::fs::read_to_string(self.paths.allowlist_file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save_allowlist(&self, rules: &[String]) {
        if let Ok(text) = serde_json::to_string_pretty(rules) {
            let _ = rig_core::fsutil::write_atomic(
                &self.paths.allowlist_file(),
                text.as_bytes(),
                0o644,
            );
        }
    }

    /// The last [`HISTORY_KEEP`] prompts. A file that has grown well past that is cut back once
    /// here, so it never grows without bound but is not rewritten on every start either.
    pub fn load_history(&self) -> Vec<String> {
        let path = self.paths.history_file();
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let keep = &lines[lines.len().saturating_sub(HISTORY_KEEP)..];
        if lines.len() > HISTORY_KEEP * 4 {
            let mut trimmed = keep.join("\n");
            trimmed.push('\n');
            let _ = rig_core::fsutil::write_atomic(&path, trimmed.as_bytes(), 0o600);
        }
        keep.iter()
            .filter_map(|l| serde_json::from_str::<String>(l).ok())
            .collect()
    }

    pub fn append_history(&self, entry: &str) {
        use std::io::Write;
        let Ok(mut line) = serde_json::to_string(entry) else {
            return;
        };
        line.push('\n');
        let _ = std::fs::create_dir_all(&self.paths.root);
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).append(true);
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        if let Ok(mut f) = opts.open(self.paths.history_file()) {
            // One write per entry, so concurrent sessions never interleave half lines.
            let _ = f.write_all(line.as_bytes());
        }
    }

    pub fn discover_skills(&self) -> Vec<Skill> {
        skills::discover(&self.cwd, self.paths.home.as_deref())
    }

    pub fn reminder(&self, skills: &[Skill]) -> Message {
        let mut extra = skills::catalog_text(skills);
        if let Some(line) = rig_mcp::catalog(&self.mcp) {
            extra.push_str(&format!("\n{line}\n"));
        }
        Message::user(session_context(&ContextInput {
            cwd: &self.cwd,
            home: self.paths.home.as_deref(),
            extra: &extra,
        }))
    }
}

#[cfg(test)]
mod tests;
