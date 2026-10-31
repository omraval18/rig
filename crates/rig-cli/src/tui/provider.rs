//! Provider and model state: credentials, the model list, per-model limits.

use super::{PendingChange, Tui};
use crate::runtime::Ev;
use rig_core::provider::Effort;

impl Tui {
    pub fn signed_in(&self) -> bool {
        self.app.has_credentials(&self.info.spec)
    }

    /// Change the agent now, or hold the change until the running turn hands the agent back.
    pub fn change(&mut self, change: PendingChange) {
        match self.agent.as_mut() {
            Some(agent) => {
                change.apply(agent);
                self.info.ctx_pct = agent.context_percent();
            }
            None => self.pending.merge(change),
        }
    }

    pub fn set_effort(&mut self, effort: Option<Effort>) {
        self.info.effort = effort;
        self.change(PendingChange {
            effort: Some(effort),
            ..PendingChange::default()
        });
    }

    /// Credentials changed (signed in or out): swap in a provider that matches and refresh the model list.
    pub fn reauth(&mut self) {
        let spec = self.info.spec.clone();
        let model = self.info.model.clone();
        let provider = match self.app.build_provider(&spec, &model) {
            Ok(p) if self.signed_in() => p,
            _ => self.app.unauthenticated(&spec, &model),
        };
        self.change(PendingChange {
            provider: Some(provider),
            ..PendingChange::default()
        });
        self.models.clear();
        self.fetch_models();
        self.dirty = true;
    }

    /// Learn the context window and supported efforts of the current model once the list arrives.
    pub(super) fn apply_model_info(&mut self) {
        let Some(m) = self
            .models
            .iter()
            .find(|m| m.id == self.info.model)
            .cloned()
        else {
            return;
        };
        let window = self.app.context_window(&self.info.spec, Some(&m));
        self.info.context_window = window;
        self.change(PendingChange {
            context_window: Some(window),
            ..PendingChange::default()
        });
        self.dirty = true;
    }

    pub fn fetch_models(&mut self) {
        let spec = self.info.spec.clone();
        if !self.app.has_credentials(&spec) {
            return;
        }
        let Ok(provider) = self.app.build_provider(&spec, &self.info.model) else {
            return;
        };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let models = provider.list_models().map_err(|e| e.to_string());
            let _ = tx.send(Ev::Models {
                provider: spec.name,
                models,
            });
        });
    }

    pub fn efforts_for_model(&self) -> Vec<Effort> {
        self.models
            .iter()
            .find(|m| m.id == self.info.model)
            .map(|m| m.efforts.clone())
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| vec![Effort::Low, Effort::Medium, Effort::High, Effort::XHigh])
    }
}
