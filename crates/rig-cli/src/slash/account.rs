//! `/provider`, `/profile`, `/login`, `/logout`.

use crate::app::{Credential, sign_in_hint};
use crate::desktop;
use crate::picker::{PickItem, PickKind};
use crate::profile;
use crate::runtime::Ev;
use crate::tui::Tui;
use rig_provider::chatgpt;

/// The `/provider` row that starts adding a provider.
const ADD_PROVIDER: &str = "add";

impl Tui {
    pub(super) fn cmd_provider(&mut self) {
        let items: Vec<PickItem> = self
            .app
            .cfg
            .providers()
            .into_iter()
            .map(|s| {
                let auth = match self.app.credential(&s) {
                    Credential::Plan { signed_in: false } => {
                        "not signed in — selecting starts sign-in".to_string()
                    }
                    Credential::Missing(var) => {
                        format!("missing ${var} · select to enter a key")
                    }
                    c => format!("ready · {}", c.describe()),
                };
                PickItem::labeled(&s.name)
                    .detail(auth)
                    .current(s.name == self.info.spec.name)
            })
            .collect();
        let mut items = items;
        items.push(
            PickItem::new("+ Add provider…", ADD_PROVIDER)
                .detail("any OpenAI- or Anthropic-compatible API"),
        );
        self.open_picker(PickKind::Provider, "Provider", items);
    }

    pub(super) fn pick_provider(&mut self, name: &str) {
        if name == ADD_PROVIDER {
            return self.open_provider_entry();
        }
        let Ok(spec) = self.app.provider_spec(name) else {
            return;
        };
        if !self.app.has_credentials(&spec) {
            if spec.chatgpt_plan {
                return self.cmd_login();
            }
            if !spec.keyless {
                return self.open_key_entry(name);
            }
            return self.error(&sign_in_hint(&spec));
        }
        let model = crate::config::default_model(name).unwrap_or("").to_string();
        if let Err(e) = self.switch_model(name, &model) {
            self.error(&e);
        }
    }

    /// `/profile` lists profiles; `/profile <name>` (or picking one) restarts rig in it.
    pub(crate) fn cmd_profile(&mut self, arg: &str) {
        let base = self.app.paths.base.clone();
        let current = self.app.paths.profile.clone();
        if arg == "new" {
            return self.open_profile_entry();
        }
        if let Some(name) = arg.strip_prefix("new ") {
            return match profile::create(&base, name.trim()) {
                Ok(_) => self.cmd_profile(name.trim()),
                Err(e) => self.error(&e),
            };
        }
        if arg.is_empty() {
            let mut items: Vec<PickItem> = profile::list(&base)
                .into_iter()
                .map(|name| {
                    PickItem::labeled(&name)
                        .detail(profile::describe(&base, &name))
                        .current(name == current)
                })
                .collect();
            // Last, so the highlight starts on the profile in use.
            items.push(PickItem::new("+ New profile…", "new").detail("create and switch"));
            return self.open_picker(PickKind::Profile, "Profile · rig restarts in it", items);
        }
        if arg == current {
            return self.say(&format!("Already using profile '{current}'."));
        }
        if self.turn.running.is_some() || self.turn.compacting {
            return self.error(super::BUSY);
        }
        if let Err(e) = profile::set_active(&base, arg) {
            return self.error(&e);
        }
        self.switch_profile = Some(arg.to_string());
        self.quit = true;
    }

    pub(super) fn cmd_login(&mut self) {
        let path = self.app.paths.chatgpt_auth_file();
        let plan = match chatgpt::LoginPlan::new() {
            Ok(plan) => plan,
            Err(e) => return self.error(&e.to_string()),
        };
        self.say(&format!("Sign in with ChatGPT in your browser (profile '{}'). If it does not open, visit:\n{}\nWaiting up to 5 minutes…", self.app.paths.profile, plan.url));
        desktop::open_url(&plan.url);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = chatgpt::login(&path, &plan, &|_| {})
                .map(|()| format!("Signed in. {}", chatgpt::status(&path).unwrap_or_default()))
                .map_err(|e| e.to_string());
            let _ = tx.send(Ev::LoginDone(r));
        });
    }

    pub(super) fn cmd_logout(&mut self) {
        let path = self.app.paths.chatgpt_auth_file();
        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.say("Signed out of ChatGPT.");
                self.reauth();
            }
            Err(_) => self.say("Not signed in."),
        }
    }
}
