//! `/model`, `/effort`, `/permissions`, `/allowlist`: what the agent runs with.

use crate::app::sign_in_hint;
use crate::picker::{PickItem, PickKind};
use crate::tui::{PendingChange, Tui, fmt_tokens};
use rig_core::permission::Autonomy;
use rig_core::provider::Effort;

use super::first_line;

impl Tui {
    pub fn open_model_picker(&mut self) {
        if self.models.is_empty() {
            // Never wait on the network here; the picker refreshes when the list arrives.
            self.fetch_models();
        }
        let items = self.model_items();
        let title = format!(
            "Model · {}  (type provider/model for anything else)",
            self.info.spec.name
        );
        self.open_picker(PickKind::Model, &title, items);
    }

    pub fn model_items(&self) -> Vec<PickItem> {
        let mut items: Vec<PickItem> = self
            .models
            .iter()
            .filter(|m| !m.hidden)
            .map(|m| {
                let ctx = m
                    .context_window
                    .map(|w| format!("{} ctx", fmt_tokens(w)))
                    .unwrap_or_default();
                let desc = first_line(&m.description, 60);
                let detail = [m.display_name.as_str(), ctx.as_str(), desc.as_str()]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" · ");
                PickItem::labeled(&m.id)
                    .detail(detail)
                    .current(m.id == self.info.model)
            })
            .collect();
        if !items.iter().any(|i| i.current) {
            items.insert(
                0,
                PickItem::labeled(&self.info.model)
                    .detail("current")
                    .current(true),
            );
        }
        for spec in self.app.cfg.providers() {
            if spec.name != self.info.spec.name
                && self.app.has_credentials(&spec)
                && let Some(m) = crate::config::default_model(&spec.name)
            {
                items.push(
                    PickItem::labeled(format!("{}/{m}", spec.name))
                        .detail(format!("switch to {}", spec.name)),
                );
            }
        }
        items
    }

    /// `arg` is `model` (current provider) or `provider/model`.
    pub(super) fn cmd_model(&mut self, arg: &str) {
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
        let p = self.app.build_provider(&spec, model)?;
        let changed_provider = spec.name != self.info.spec.name;
        self.info.spec = spec;
        self.info.model = model.to_string();
        if changed_provider {
            self.models.clear();
            self.fetch_models();
        }
        let known = self.models.iter().find(|m| m.id == model).cloned();
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
