//! Values that come from flags, config or last-used state: effort, permissions, context window.

use crate::config::default_context_window;
use rig_core::permission::Autonomy;
use rig_core::provider::{Effort, ModelInfo};
use rig_provider::ProviderSpec;

use super::App;

impl App {
    pub fn effort(&self, flag: Option<&str>) -> Result<Option<Effort>, String> {
        match flag
            .or(self.state.last(&self.state.effort))
            .or(self.cfg.effort.as_deref())
        {
            None => Ok(Some(Effort::Medium)),
            Some("none" | "off") => Ok(None),
            Some(s) => Effort::parse(s)
                .map(Some)
                .ok_or_else(|| format!("unknown effort '{s}' (low, medium, high, xhigh, none)")),
        }
    }

    /// Scripts (`interactive` false) never inherit a permission level you set in a chat once.
    pub fn autonomy(&self, flag: Option<&str>, interactive: bool) -> Result<Autonomy, String> {
        let saved = self
            .state
            .last(&self.state.autonomy)
            .filter(|_| interactive);
        match flag.or(saved).or(self.cfg.autonomy.as_deref()) {
            None => Ok(Autonomy::Low),
            Some(s) => Autonomy::parse(s).ok_or_else(|| {
                format!("unknown autonomy '{s}' (ask, read-only, low, medium, high)")
            }),
        }
    }

    pub fn context_window(&self, spec: &ProviderSpec, info: Option<&ModelInfo>) -> u64 {
        self.cfg
            .context_window
            .or(info.and_then(|i| i.context_window))
            .unwrap_or_else(|| default_context_window(&spec.name))
    }

    pub fn auto_compact_percent(&self) -> u8 {
        self.state
            .auto_compact_percent
            .or(self.cfg.auto_compact_percent)
            .unwrap_or(80)
            .clamp(10, 80)
    }
}
