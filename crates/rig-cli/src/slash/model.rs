//! `/model`, `/effort`, `/permissions`, `/allowlist`: what the agent runs with.

use crate::app::sign_in_hint;
use crate::picker::{CREATE, Create, PickItem, PickKind, Picker};
use crate::tui::{PendingChange, Tui, fmt_tokens};
use rig_core::permission::Autonomy;
use rig_core::provider::{Effort, ModelInfo};
use rig_provider::ProviderSpec;

use super::first_line;

/// The model picker's row that connects a provider.
const CONNECT: &str = "+connect";

/// Name, context window and list price (or that a plan covers it).
pub(crate) fn model_detail(spec: &ProviderSpec, m: &ModelInfo) -> String {
    let name = Some(m.display_name.as_str()).filter(|n| !n.eq_ignore_ascii_case(&m.id));
    let ctx = m.context_window.map(|w| format!("{} ctx", fmt_tokens(w)));
    let price = if spec.chatgpt_plan || spec.subscription {
        Some("plan".to_string())
    } else {
        rig_core::pricing::lookup(&spec.name, &m.id)
            .map(|p| format!("${}/${} per M", p.input, p.output))
    };
    let about = first_line(&m.description, 60);
    [name.map(String::from), ctx, price, Some(about)]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

impl Tui {
    /// Every model of every connected provider. Opens with the lists known so far and fills in as
    /// fresh ones arrive; it never waits on the network.
    pub fn open_model_picker(&mut self) {
        self.show_model_picker(None, "");
    }

    /// One provider's models only.
    pub fn open_model_picker_for(&mut self, provider: &str) {
        self.show_model_picker(Some(provider), "");
    }

    fn show_model_picker(&mut self, scope: Option<&str>, filter: &str) {
        let items = self.model_items(scope);
        if !items.iter().any(|i| i.value != CONNECT) {
            return match scope {
                // No list yet and no default: the picker opens when the list arrives.
                Some(name) => {
                    self.choose_from = Some(name.to_string());
                    self.notice(format!("getting {name}'s models…"));
                }
                None => self.open_provider_picker(""),
            };
        }
        let title = match scope {
            Some(name) => format!("Model · {name}"),
            None => "Model · all connected providers".into(),
        };
        self.open_picker(PickKind::Model, &title, items);
        if let Some(p) = self.prompt.picker.as_mut() {
            p.scope = scope.map(String::from);
            p.set_create(Create::Model);
            p.set_filter(filter);
        }
        let stale = self.connected_specs();
        let stale = stale
            .into_iter()
            .filter(|s| scope.is_none_or(|n| n == s.name) && self.models.is_stale(s))
            .collect();
        self.refresh_models(stale, false);
    }

    /// Update the open model picker, if there is one.
    pub fn refresh_model_picker(&mut self) {
        let Some(p) = self.prompt.picker.as_ref() else {
            return;
        };
        if p.kind != PickKind::Model {
            return;
        }
        let items = self.model_items(p.scope.clone().as_deref());
        if let Some(p) = self.prompt.picker.as_mut() {
            p.replace_items(items);
        }
    }

    /// Rows for the models of `scope`, or of every connected provider: recent ones, then each
    /// provider's under its own heading, the current provider first. Every value is `provider/model`.
    pub fn model_items(&self, scope: Option<&str>) -> Vec<PickItem> {
        let mut specs = self.connected_specs();
        specs.retain(|s| scope.is_none_or(|n| n == s.name));
        specs.sort_by_key(|s| s.name != self.info.spec.name);
        let mut items = Vec::new();
        for spec in &specs {
            items.extend(self.provider_models(spec));
        }
        if scope.is_none() {
            let mut recent: Vec<PickItem> = Vec::new();
            for value in &self.app.state.recent_models {
                if let Some(i) = items.iter().position(|it| it.value == *value) {
                    recent.push(items.remove(i).group("Recent"));
                }
            }
            recent.extend(items);
            items = recent;
            items.push(
                PickItem::new("+ Connect a provider…", CONNECT)
                    .detail("sign in, or enter a key")
                    .group("More"),
            );
        }
        items
    }

    fn provider_models(&self, spec: &ProviderSpec) -> Vec<PickItem> {
        let here = spec.name == self.info.spec.name;
        let listing = self.models.get(spec);
        let mut models: Vec<ModelInfo> = self.models.of(spec).to_vec();
        models.retain(|m| !m.hidden);
        // The model in use and the default are offered even when the list is missing them.
        let default = self
            .app
            .default_model_of(spec)
            .filter(|_| spec.default_model.is_some() || models.is_empty())
            // A local server that is not running has nothing to offer.
            .filter(|_| !spec.keyless || !models.is_empty() || here);
        let current = here.then(|| self.info.model.clone());
        for id in current.into_iter().chain(default).filter(|m| !m.is_empty()) {
            if !models.iter().any(|m| m.id == id) {
                let id = id.clone();
                models.insert(
                    0,
                    ModelInfo {
                        id,
                        ..ModelInfo::default()
                    },
                );
            }
        }
        let state = match listing {
            Some(l) if l.loading() => "refreshing…".to_string(),
            Some(l) => match &l.error {
                Some(e) => first_line(e, 50),
                None => format!("{} models", models.len()),
            },
            None => "list not loaded yet".into(),
        };
        let group = format!("{} · {state}", spec.name);
        models
            .iter()
            .map(|m| {
                let value = format!("{}/{}", spec.name, m.id);
                PickItem::labeled(&value)
                    .detail(model_detail(spec, m))
                    .current(here && m.id == self.info.model)
                    .group(&group)
            })
            .collect()
    }

    pub(super) fn pick_model(&mut self, value: &str) {
        if value == CONNECT {
            return self.open_provider_picker("");
        }
        if let Some(typed) = value.strip_prefix(CREATE) {
            return self.use_model_as_typed(typed);
        }
        if let Some((provider, model)) = value.split_once('/')
            && let Err(e) = self.switch_model(provider, model)
        {
            self.error(&e);
        }
    }

    /// `provider/model`, a model id, `-` for the previous model, or words to search for.
    pub(super) fn cmd_model(&mut self, arg: &str) {
        let current = format!("{}/{}", self.info.spec.name, self.info.model);
        if arg == "-" {
            let recent = &self.app.state.recent_models;
            return match recent.iter().find(|m| **m != current).cloned() {
                Some(previous) => self.pick_model(&previous),
                None => self.say("No earlier model to go back to."),
            };
        }
        let items = self.model_items(None);
        let wanted = format!("/{arg}");
        let exact: Vec<String> = items
            .iter()
            .map(|i| &i.value)
            .filter(|v| *v == arg || v.split_once('/').is_some_and(|(_, m)| m == arg))
            .cloned()
            .collect();
        let preferred = exact
            .iter()
            .find(|v| *v == arg)
            .or_else(|| {
                let here = format!("{}{wanted}", self.info.spec.name);
                exact.iter().find(|v| **v == here)
            })
            .or(exact.first().filter(|_| exact.len() == 1));
        if let Some(value) = preferred {
            return self.pick_model(&value.clone());
        }
        let names_provider = arg
            .split_once('/')
            .is_some_and(|(p, m)| !m.is_empty() && self.app.provider_spec(p).is_ok());
        let mut search = Picker::new(PickKind::Model, "", items);
        search.set_filter(arg);
        if !names_provider && search.selected_item().is_some() {
            return self.show_model_picker(None, arg);
        }
        self.use_model_as_typed(arg);
    }

    /// A model no list has: `provider/model`, or an id for the current provider.
    fn use_model_as_typed(&mut self, arg: &str) {
        let (provider, model) = match arg.split_once('/') {
            Some((p, m)) if self.app.provider_spec(p).is_ok() && !m.is_empty() => {
                (p.to_string(), m.to_string())
            }
            _ => (self.info.spec.name.clone(), arg.to_string()),
        };
        if let Err(e) = self.switch_model(&provider, &model) {
            self.error(&e);
        }
    }

    pub fn switch_model(&mut self, provider: &str, model: &str) -> Result<(), String> {
        let spec = self.app.provider_spec(provider)?;
        if !self.app.has_credentials(&spec) {
            return Err(sign_in_hint(&spec));
        }
        if model.is_empty() {
            return Err(format!("choose a model for {provider} with /model"));
        }
        let p = self.app.build_provider(&spec, model)?;
        let previous = format!("{}/{}", self.info.spec.name, self.info.model);
        if self.signed_in() && !self.info.model.is_empty() {
            self.app.state.touch_model(&previous);
        }
        self.app.state.touch_model(&format!("{provider}/{model}"));
        self.info.spec = spec;
        self.info.model = model.to_string();
        if self.models.is_stale(&self.info.spec) {
            self.refresh_models(vec![self.info.spec.clone()], false);
        }
        let known = self
            .current_models()
            .iter()
            .find(|m| m.id == model)
            .cloned();
        let window = self.app.context_window(&self.info.spec, known.as_ref());
        self.info.context_window = window;
        let efforts = self.efforts_for_model();
        if let Some(e) = self.info.effort
            && !efforts.contains(&e)
        {
            self.info.effort = known
                .and_then(|m| m.default_effort)
                .or(Some(Effort::Medium));
        }
        self.change(PendingChange {
            provider: Some(p),
            context_window: Some(window),
            effort: Some(self.info.effort),
            ..PendingChange::default()
        });
        self.remember_model();
        self.remember_effort();
        let model = self.kv(
            "Model:",
            &format!("{}/{}", self.info.spec.name, self.info.model),
            crate::style::CYAN,
        );
        let later = if self.agent.is_none() {
            self.dim(" (applies from the next turn)")
        } else {
            String::new()
        };
        self.out(&format!("{model}{later}"));
        Ok(())
    }

    pub(super) fn cmd_effort(&mut self, arg: &str) {
        if arg.is_empty() {
            let current = self.info.effort;
            let mut items: Vec<PickItem> = self
                .efforts_for_model()
                .into_iter()
                .map(|e| PickItem::labeled(e.as_str()).current(Some(e) == current))
                .collect();
            items.push(
                PickItem::labeled("none")
                    .detail("do not request reasoning")
                    .current(current.is_none()),
            );
            return self.open_picker(PickKind::Effort, "Reasoning effort", items);
        }
        let effort = match arg {
            "none" | "off" => None,
            other => match Effort::parse(other) {
                Some(e) => Some(e),
                None => return self.error("effort must be low, medium, high, xhigh or none"),
            },
        };
        self.set_effort(effort);
        self.remember_effort();
        let line = self.kv(
            "Reasoning effort:",
            effort.map_or("none", Effort::as_str),
            crate::style::VIOLET,
        );
        self.out(&line);
    }

    pub(super) fn cmd_permissions(&mut self, arg: &str) {
        if arg.is_empty() {
            let cur = self.policy.autonomy();
            let items = Autonomy::ALL
                .iter()
                .map(|a| {
                    PickItem::labeled(a.as_str())
                        .detail(a.describe())
                        .current(*a == cur)
                })
                .collect();
            return self.open_picker(
                PickKind::Permissions,
                "What may rig do without asking?",
                items,
            );
        }
        match Autonomy::parse(arg) {
            Some(a) => {
                self.policy.set(a);
                self.remember_autonomy();
                let line = format!("{} {}", self.kv("Permissions:", a.as_str(), crate::style::permission(a)), self.dim(&format!("— {}", a.describe())));
                self.out(&line);
            }
            None => self.error("permissions must be ask, read-only, low, medium or high (auto and full-access also work)"),
        }
    }

    pub fn cycle_permissions(&mut self) {
        let cur = self.policy.autonomy();
        let i = Autonomy::ALL.iter().position(|a| *a == cur).unwrap_or(0);
        let next = Autonomy::ALL[(i + 1) % Autonomy::ALL.len()];
        self.policy.set(next);
        self.remember_autonomy();
        self.notice(format!(
            "permissions: {} · {}",
            next.as_str(),
            next.describe()
        ));
    }

    pub(super) fn cmd_allowlist(&mut self, arg: &str) {
        let rules = self.policy.allowlist();
        let mut parts = arg.split_whitespace();
        match parts.next().unwrap_or("view") {
            "view" | "list" => {
                if rules.is_empty() {
                    self.say("No always-allowed rules yet. Choose \"Always\" at an approval prompt to add one.");
                } else {
                    let list = rules
                        .iter()
                        .enumerate()
                        .map(|(i, r)| format!("  {}. {r}", i + 1))
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.say(&format!("Always allowed:\n{list}"));
                }
            }
            "remove" => match parts
                .next()
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|n| *n >= 1 && *n <= rules.len())
            {
                Some(n) => {
                    let mut r = rules;
                    let gone = r.remove(n - 1);
                    self.policy.set_allowlist(r.clone());
                    self.app.save_allowlist(&r);
                    self.say(&format!("Removed: {gone}"));
                }
                None => self.say("Usage: /allowlist remove <number>"),
            },
            "reset" => {
                self.policy.set_allowlist(Vec::new());
                self.app.save_allowlist(&[]);
                self.say("Cleared the always-allowed list.");
            }
            _ => self.say("Usage: /allowlist [view|remove <n>|reset]"),
        }
    }
}
