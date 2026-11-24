//! Provider and model state: credentials, the model list, per-model limits.

use super::{PendingChange, Tui};
use crate::runtime::Ev;
use rig_core::provider::{Effort, ModelInfo};
use rig_provider::ProviderSpec;

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
        self.models.forget(&spec.name);
        if self.signed_in() {
            self.refresh_models(vec![spec], false);
        }
        self.dirty = true;
    }

    /// Learn the context window and supported efforts of the current model once the list arrives.
    pub(super) fn apply_model_info(&mut self) {
        let Some(m) = self
            .current_models()
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

    /// The model list of the provider in use, as far as it is known.
    pub fn current_models(&self) -> &[ModelInfo] {
        self.models.of(&self.info.spec)
    }

    pub fn connected_specs(&self) -> Vec<ProviderSpec> {
        let mut specs = self.app.cfg.providers();
        specs.retain(|s| self.app.has_credentials(s));
        specs
    }

    /// Fetch these providers' model lists in the background. `report` says what came back.
    pub fn refresh_models(&mut self, specs: Vec<ProviderSpec>, report: bool) {
        if report {
            if specs.is_empty() {
                return self.say("No provider is connected yet: /provider connects one.");
            }
            self.notice("refreshing models…");
        }
        for spec in specs {
            let lister = match self.app.model_lister(&spec) {
                Ok(lister) => lister,
                Err(e) => {
                    self.notice(e);
                    continue;
                }
            };
            if report {
                self.reporting.insert(spec.name.clone());
            }
            let seq = self.models.begin(&spec);
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Ev::Models {
                    provider: spec.name,
                    seq,
                    models: lister(),
                });
            });
        }
        self.refresh_model_picker();
    }

    pub(super) fn on_models(
        &mut self,
        provider: &str,
        seq: u64,
        models: Result<Vec<ModelInfo>, String>,
    ) {
        let Some(result) = self.models.finish(provider, seq, models) else {
            return;
        };
        let reported = self.reporting.remove(provider);
        let choose = self.choose_from.take_if(|p| p == provider).is_some();
        let spec = self.app.provider_spec(provider).ok();
        // A local server that has never answered is simply not running.
        let absent = spec.is_some_and(|s| s.keyless && self.models.of(&s).is_empty());
        let here = provider == self.info.spec.name;
        let failed = result.is_err();
        match result {
            Ok(update) => {
                let new = match update.added.as_slice() {
                    [] => String::new(),
                    [first, rest @ ..] if rest.len() > 3 => {
                        format!(" · new: {first} and {} more", rest.len())
                    }
                    some => format!(" · new: {}", some.join(", ")),
                };
                if reported {
                    self.say(&format!("{provider}: {} models{new}", update.total));
                } else if here && !new.is_empty() {
                    self.notice(format!("{provider}{new}"));
                }
                if here {
                    self.apply_model_info();
                }
            }
            Err(e) => {
                let short: String = e.chars().take(120).collect();
                let rejected = e.ends_with("rejected these credentials");
                if rejected || choose || (reported && (here || !absent)) {
                    self.error(&format!("{provider}: {short}"));
                } else if here && self.current_models().is_empty() {
                    self.notice(format!("could not list models: {short}"));
                }
            }
        }
        self.refresh_model_picker();
        self.refresh_provider_picker();
        if choose && !failed && self.prompt.is_free() {
            self.open_model_picker_for(provider);
        }
        self.dirty = true;
    }

    pub fn efforts_for_model(&self) -> Vec<Effort> {
        self.current_models()
            .iter()
            .find(|m| m.id == self.info.model)
            .map(|m| m.efforts.clone())
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| vec![Effort::Low, Effort::Medium, Effort::High, Effort::XHigh])
    }
}
