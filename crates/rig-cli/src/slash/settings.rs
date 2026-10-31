//! `/settings`, and remembering what you chose (last used and per session).

use crate::picker::{PickItem, PickKind};
use crate::tui::{PendingChange, Tui};
use rig_core::permission::Autonomy;
use rig_core::provider::Effort;
use rig_core::session::Session;
use rig_core::sync::MutexExt;

impl Tui {
    pub(super) fn cmd_settings(&mut self, arg: &str) {
        match arg {
            "" => self.open_settings(None),
            "files" | "paths" => self.settings_files(),
            other => self.error(&format!(
                "unknown settings view '{other}'. Try /settings or /settings files."
            )),
        }
    }

    pub(super) fn settings_files(&mut self) {
        let files: Vec<String> = self
            .app
            .paths
            .config_files(&self.app.cwd)
            .iter()
            .map(|f| {
                format!(
                    "  {} {}",
                    f.display(),
                    if f.is_file() { "(loaded)" } else { "(absent)" }
                )
            })
            .collect();
        let msg = format!(
            "Config files (later wins; what you picked last beats them unless you turned remembering off):\n{}\nKeys: model, effort, autonomy, max_turns, context_window, auto_compact_percent, [providers.<name>]\nLast-used choices: {}",
            files.join("\n"),
            self.app.paths.state_file().display()
        );
        self.say(&msg);
    }

    pub(super) fn remember_model(&mut self) {
        self.app.state.model = Some(format!("{}/{}", self.info.spec.name, self.info.model));
        self.app.save_state();
        self.record_session_settings();
    }

    pub(super) fn remember_effort(&mut self) {
        self.app.state.effort = Some(self.info.effort.map_or("none", Effort::as_str).to_string());
        self.app.save_state();
        self.record_session_settings();
    }

    pub(super) fn remember_autonomy(&mut self) {
        self.app.state.autonomy = Some(self.policy.autonomy().as_str().to_string());
        self.app.save_state();
        self.record_session_settings();
    }

    /// Every session keeps its own model, effort and permissions, so resuming it brings them back
    /// whatever other sessions used since.
    pub fn record_session_settings(&mut self) {
        let model = format!("{}/{}", self.info.spec.name, self.info.model);
        let effort = self.info.effort.map(Effort::as_str);
        let _ =
            self.session
                .locked()
                .append_settings(&model, effort, self.policy.autonomy().as_str());
    }

    /// Put back what a session was using. Anything it never recorded is left as it is.
    pub(super) fn restore_session_settings(&mut self, path: &std::path::Path) {
        let own = Session::settings(path);
        if let Some(m) = own.model.as_deref()
            && let Some((provider, model)) = m.split_once('/')
            && (provider != self.info.spec.name || model != self.info.model)
            && let Err(e) = self.switch_model(provider, model)
        {
            self.notice(format!("could not restore {m}: {e}"));
        }
        if let Some(e) = own.effort.as_deref() {
            let effort = if e == "none" { None } else { Effort::parse(e) };
            self.set_effort(effort);
        }
        if let Some(a) = own.autonomy.as_deref().and_then(Autonomy::parse) {
            self.policy.set(a);
        }
        self.record_session_settings();
    }

    const COMPACT_STEPS: [u8; 5] = [40, 50, 60, 70, 80];

    pub(super) fn settings_items(&self) -> Vec<PickItem> {
        let access = self.policy.autonomy();
        let row =
            |label: &str, detail: String, value: &str| PickItem::new(label, value).detail(detail);
        vec![
            row(
                "Model",
                format!("{}/{}", self.info.spec.name, self.info.model),
                "model",
            ),
            row(
                "Reasoning effort",
                self.info.effort.map_or("off", Effort::as_str).to_string(),
                "effort",
            ),
            row(
                "Permissions",
                format!("{} · {}", access.as_str(), access.describe()),
                "permissions",
            ),
            row(
                "Reasoning summaries",
                if self.show_reasoning {
                    "on · shown while the model thinks".to_string()
                } else {
                    "off · hidden".to_string()
                },
                "reasoning",
            ),
            row(
                "Auto-compact at",
                format!("{}% of the context window", self.app.auto_compact_percent()),
                "compact",
            ),
            row(
                "Remember last settings",
                if self.app.state.remember {
                    "on · new chats start with your last model, effort and permissions".to_string()
                } else {
                    "off · new chats start from config.toml".to_string()
                },
                "remember",
            ),
            row(
                "Mouse wheel scrolling",
                if self.app.state.mouse.unwrap_or(true) {
                    "on · scroll the conversation; hold Shift (Option on macOS) to select text"
                        .to_string()
                } else {
                    "off · use PageUp/PageDown to scroll; select text freely".to_string()
                },
                "mouse",
            ),
            row(
                "Config files",
                "show where settings are read from".into(),
                "files",
            ),
        ]
    }

    /// Open the settings list, with the highlight on `focus` (so a toggle leaves you where you were).
    pub fn open_settings(&mut self, focus: Option<&str>) {
        let items = self.settings_items();
        self.open_picker(
            PickKind::Settings,
            "Settings · Enter changes the highlighted one",
            items,
        );
        if let (Some(f), Some(p)) = (focus, self.prompt.picker.as_mut()) {
            p.select_value(f);
        }
    }

    pub(super) fn pick_setting(&mut self, key: &str) {
        match key {
            "model" => self.open_model_picker(),
            "effort" => self.cmd_effort(""),
            "permissions" => self.cmd_permissions(""),
            "reasoning" => {
                self.toggle_reasoning();
                self.open_settings(Some("reasoning"));
            }
            "compact" => {
                let now = self.app.auto_compact_percent();
                let next = Self::COMPACT_STEPS
                    .iter()
                    .copied()
                    .find(|s| *s > now)
                    .unwrap_or(Self::COMPACT_STEPS[0]);
                self.app.state.auto_compact_percent = Some(next);
                self.app.save_state();
                self.change(PendingChange {
                    auto_compact_percent: Some(next),
                    ..PendingChange::default()
                });
                self.open_settings(Some("compact"));
            }
            "remember" => {
                self.app.state.remember = !self.app.state.remember;
                if self.app.state.remember {
                    // Turning it back on records what is in effect now.
                    self.remember_model();
                    self.remember_effort();
                    self.remember_autonomy();
                } else {
                    self.app.save_state();
                }
                self.open_settings(Some("remember"));
            }
            "mouse" => {
                let on = !self.app.state.mouse.unwrap_or(true);
                self.app.state.mouse = Some(on);
                self.app.save_state();
                let _ = if on {
                    crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)
                } else {
                    crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture)
                };
                self.open_settings(Some("mouse"));
            }
            "files" => self.settings_files(),
            _ => {}
        }
    }

    fn toggle_reasoning(&mut self) {
        self.show_reasoning = !self.show_reasoning;
        self.app.state.show_reasoning = Some(self.show_reasoning);
        self.app.save_state();
    }

    pub(super) fn cmd_reasoning(&mut self) {
        self.toggle_reasoning();
        self.say(if self.show_reasoning {
            "Reasoning summaries: shown."
        } else {
            "Reasoning summaries: hidden."
        });
    }
}
