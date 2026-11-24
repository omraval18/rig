//! `/provider`: connect a provider, and manage the connected ones.

use crate::app::Credential;
use crate::catalog::{self, Catalog};
use crate::picker::{CREATE, Create, PickItem, PickKind};
use crate::runtime::Ev;
use crate::tui::Tui;
use rig_core::text::count;

/// Rows that are not a provider.
const CUSTOM: &str = "custom";
const SEARCH: &str = "search";
/// What the other rows' values start with: a provider rig knows, or one from models.dev.
const KNOWN: &str = "known:";
const LISTED: &str = "listed:";

/// A models.dev provider rig can talk to, with nothing left to fill in but a key.
pub(crate) fn connectable<'a>(
    cat: &'a Catalog,
    name: &str,
) -> Option<(&'a catalog::Provider, &'static str, String)> {
    let p = cat.providers.get(name)?;
    let kind = catalog::kind_for(p.npm.as_deref()?)?;
    // Some URLs have a `${ACCOUNT}` to fill in.
    let url = catalog::base_url(p, kind).filter(|u| !u.contains("${"))?;
    crate::profile::validate(name).ok()?;
    Some((p, kind, url))
}

impl Tui {
    pub(super) fn cmd_provider(&mut self, name: &str) {
        match self.app.provider_spec(name) {
            Ok(_) => self.pick_known(name),
            Err(_) => self.open_provider_picker(name),
        }
    }

    pub(crate) fn open_provider_picker(&mut self, filter: &str) {
        let saved = catalog::load(&self.app.paths.base);
        if !saved.as_ref().is_some_and(catalog::is_fresh) {
            // The rows from models.dev join the list when the download is done.
            let base = self.app.paths.base.clone();
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                if catalog::ensure(&base).is_ok() {
                    let _ = tx.send(Ev::Catalog);
                }
            });
        }
        let items = self.provider_items(saved.as_ref());
        self.open_picker(PickKind::Provider, "Providers", items);
        if let Some(p) = self.prompt.picker.as_mut() {
            p.set_create(Create::Endpoint);
            p.set_filter(filter);
        }
    }

    pub(crate) fn refresh_provider_picker(&mut self) {
        let open = |t: &Self| {
            t.prompt
                .picker
                .as_ref()
                .is_some_and(|p| p.kind == PickKind::Provider)
        };
        if open(self) {
            let items = self.provider_items(catalog::load(&self.app.paths.base).as_ref());
            if let Some(p) = self.prompt.picker.as_mut() {
                p.replace_items(items);
            }
        }
    }

    fn provider_items(&self, cat: Option<&Catalog>) -> Vec<PickItem> {
        let specs = self.app.cfg.providers();
        let (mut connected, mut available) = (Vec::new(), Vec::new());
        for s in &specs {
            let credential = self.app.credential(s);
            let row = PickItem::new(&s.name, format!("{KNOWN}{}", s.name));
            // A local server counts as connected once it has answered.
            let answered = !s.keyless || !self.models.of(s).is_empty();
            if credential.usable() && answered {
                let models = self.models.of(s).iter().filter(|m| !m.hidden).count();
                let detail = match models {
                    0 => credential.describe(),
                    n => format!("{} · {}", credential.describe(), count(n, "model")),
                };
                let row = row.detail(detail).group("Connected");
                connected.push(row.current(s.name == self.info.spec.name));
            } else {
                let detail = match credential {
                    Credential::Plan { .. } => "sign in with your browser".to_string(),
                    Credential::Missing(var) => format!("enter a key (or set ${var})"),
                    _ => format!("local · {}", s.base_url),
                };
                available.push(row.detail(detail).group("Connect"));
            }
        }
        // The subscription most people have first, then by name.
        available.sort_by_key(|i| i.label != "chatgpt");
        let mut items = connected;
        items.extend(available);
        let more = |label: &str, value: &str, detail: &str| {
            PickItem::new(label, value).detail(detail).group("Connect")
        };
        items.push(more(
            "+ Custom endpoint…",
            CUSTOM,
            "paste a URL; rig finds the API style and models",
        ));
        items.push(more(
            "Search models by name…",
            SEARCH,
            "on models.dev, with prices",
        ));
        if let Some(cat) = cat {
            items.extend(
                cat.providers
                    .keys()
                    .filter(|name| !specs.iter().any(|s| s.name == **name))
                    .filter_map(|name| {
                        let (p, _, _) = connectable(cat, name)?;
                        Some(
                            PickItem::new(name, format!("{LISTED}{name}"))
                                .detail(format!("{} · {}", p.name, count(p.models.len(), "model")))
                                .group("More from models.dev"),
                        )
                    }),
            );
        }
        items
    }

    pub(super) fn pick_provider(&mut self, value: &str) {
        match value {
            CUSTOM => self.open_provider_entry(),
            SEARCH => self.open_catalog_search(),
            _ => {
                if let Some(url) = value.strip_prefix(CREATE) {
                    self.open_provider_entry();
                    if !url.is_empty() {
                        self.give_entry(url);
                    }
                } else if let Some(name) = value.strip_prefix(KNOWN) {
                    self.pick_known(name);
                } else if let Some(name) = value.strip_prefix(LISTED) {
                    self.connect_listed(name);
                }
            }
        }
    }

    fn pick_known(&mut self, name: &str) {
        let Ok(spec) = self.app.provider_spec(name) else {
            return;
        };
        let credential = self.app.credential(&spec);
        match &credential {
            Credential::Plan { signed_in: false } => return self.login_chatgpt(),
            Credential::Missing(_) => return self.open_key_entry(name),
            _ => {}
        }
        if spec.keyless && self.models.of(&spec).is_empty() {
            self.say(&format!("Looking for models at {}…", spec.base_url));
            return self.connected(name);
        }
        let row = |label: &str, action: &str, detail: String| {
            PickItem::new(label, format!("{action}:{name}")).detail(detail)
        };
        let mut items = vec![
            row("Choose a model", "models", String::new()),
            row("Refresh models", "refresh", String::new()),
        ];
        match &credential {
            Credential::Plan { .. } => items.push(row(
                "Sign in again",
                "login",
                "or to another account".into(),
            )),
            Credential::Saved | Credential::Env(_) => {
                items.push(row("Replace key", "key", credential.describe()));
            }
            _ => {}
        }
        let added = crate::providers::load(&self.app.paths.providers_file()).contains_key(name);
        let saved = matches!(credential, Credential::Saved | Credential::Plan { .. });
        if added {
            items.push(row(
                "Remove",
                "disconnect",
                "forget this provider and its key".into(),
            ));
        } else if saved {
            items.push(row("Disconnect", "disconnect", "sign out here".into()));
        }
        self.open_picker(PickKind::ProviderAction, name, items);
    }

    pub(super) fn provider_action(&mut self, value: &str) {
        let Some((action, name)) = value.split_once(':') else {
            return;
        };
        match action {
            "models" => self.open_model_picker_for(name),
            "refresh" => {
                if let Ok(spec) = self.app.provider_spec(name) {
                    self.refresh_models(vec![spec], true);
                }
            }
            "login" => self.login_chatgpt(),
            "key" => self.open_key_entry(name),
            "disconnect" => self.disconnect(name),
            _ => {}
        }
    }

    /// Credentials for `name` are in place: get its models and let the user choose one.
    pub(crate) fn connected(&mut self, name: &str) {
        self.models.forget(name);
        if name == self.info.spec.name {
            if let Ok(spec) = self.app.provider_spec(name) {
                self.info.spec = spec;
            }
            self.reauth();
        } else if let Ok(spec) = self.app.provider_spec(name) {
            // Nothing usable yet: start on this provider's default model right away.
            if !self.signed_in()
                && let Some(model) = self.app.default_model_of(&spec)
                && let Err(e) = self.switch_model(name, &model)
            {
                self.error(&e);
            }
            self.refresh_models(vec![spec], false);
        }
        // Never take the keyboard from someone who is typing or answering something.
        if self.prompt.is_free() {
            self.open_model_picker_for(name);
        }
    }

    /// Forget a provider's login or key, and the provider itself when it was added from the UI.
    pub(super) fn disconnect(&mut self, name: &str) {
        let spec = match self.app.provider_spec(name) {
            Ok(spec) => spec,
            Err(e) => return self.error(&e),
        };
        let mut done = false;
        if spec.chatgpt_plan {
            done = std::fs::remove_file(self.app.paths.chatgpt_auth_file()).is_ok();
        }
        if self.app.keys.remove(name) {
            done = true;
            if let Err(e) = self.app.keys.save(&self.app.paths.keys_file()) {
                return self.error(&e);
            }
        }
        match crate::providers::remove(&self.app.paths.providers_file(), name) {
            Ok(true) => {
                done = true;
                self.app.cfg.providers.remove(name);
            }
            Ok(false) => {}
            Err(e) => return self.error(&e),
        }
        if !done {
            return self.say(&format!(
                "{name} has no login or saved key in this profile."
            ));
        }
        self.models.forget(name);
        self.say(&format!("Disconnected {name}."));
        let still_works = self
            .app
            .provider_spec(name)
            .is_ok_and(|s| self.app.has_credentials(&s));
        if name != self.info.spec.name {
            return;
        }
        // Carry on with another connected provider, if there is one.
        match self.app.resolve_model(None) {
            Ok((next, model)) if !still_works && self.app.has_credentials(&next) => {
                if let Err(e) = self.switch_model(&next.name, &model) {
                    self.error(&e);
                }
            }
            _ => self.reauth(),
        }
    }
}
