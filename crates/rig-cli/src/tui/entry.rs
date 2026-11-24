//! The text box over the composer: typing an API key, or naming a new profile.

use super::Tui;
use crate::app::App;
use crate::catalog;
use crate::entry::{Entry, Purpose};
use crate::picker::{PickItem, PickKind};
use crate::probe;
use crate::profile;
use crate::runtime::Ev;
use crate::slash::provider::connectable;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rig_core::provider::ModelInfo;
use rig_provider::{Kind, ProviderSpec};

impl Tui {
    /// Ask for a provider's API key. Enter saves it in the active profile and lists the models.
    pub(crate) fn open_key_entry(&mut self, provider: &str) {
        let profile = self.app.paths.profile.clone();
        self.open_entry(Entry {
            purpose: Purpose::ApiKey {
                provider: provider.to_string(),
            },
            title: format!("API key for {provider}"),
            hint: format!("saved in profile '{profile}' (keys.json, only you can read it)"),
            value: String::new(),
            secret: true,
        });
    }

    /// Ask for a name for a new profile. Enter creates it and switches to it.
    pub(crate) fn open_profile_entry(&mut self) {
        self.open_entry(Entry {
            purpose: Purpose::NewProfile,
            title: "New profile name".into(),
            hint: "letters, digits, '-' and '_'; it gets its own logins and keys".into(),
            value: String::new(),
            secret: false,
        });
    }

    /// Add an endpoint by URL. Its key is asked next; the API style, models and name are found out.
    pub(crate) fn open_provider_entry(&mut self) {
        self.open_entry(Entry {
            purpose: Purpose::ProviderUrl,
            title: "Base URL of the API".into(),
            hint: "such as https://api.example.com/v1 or http://localhost:8000".into(),
            value: String::new(),
            secret: false,
        });
    }

    /// Fill in the open box and move on, as if it had been typed.
    pub(crate) fn give_entry(&mut self, value: &str) {
        if let Some(e) = self.prompt.entry.as_mut() {
            e.value = value.to_string();
        }
        self.finish_entry();
    }

    /// Connect a provider from models.dev: at once when its key is in the environment.
    pub(crate) fn connect_listed(&mut self, name: &str) {
        if let Err(e) = self.start_listed(name, None) {
            self.error(&e);
        }
    }

    fn start_listed(&mut self, name: &str, model: Option<&str>) -> Result<(), String> {
        let cat = catalog::ensure(&self.app.paths.base)?;
        let (p, _, _) = connectable(&cat, name)
            .ok_or_else(|| format!("rig does not speak the API of {name} yet"))?;
        let set = |v: &&String| App::getenv(v).is_some_and(|k| !k.trim().is_empty());
        if p.env.iter().any(|v| set(&v)) {
            return self.save_listed(name, "", model);
        }
        self.open_entry(Entry {
            purpose: Purpose::ListedKey {
                name: name.into(),
                model: model.map(String::from),
            },
            title: format!("API key for {}", p.name),
            hint: "stored in keys.json, only you can read it".into(),
            value: String::new(),
            secret: true,
        });
        Ok(())
    }

    fn save_listed(&mut self, name: &str, key: &str, model: Option<&str>) -> Result<(), String> {
        let cat = catalog::ensure(&self.app.paths.base)?;
        let (p, kind, url) =
            connectable(&cat, name).ok_or_else(|| format!("{name} is no longer listed"))?;
        let kind = if kind == "anthropic" {
            Kind::Anthropic
        } else {
            Kind::Chat
        };
        let set = |v: &&String| App::getenv(v).is_some_and(|k| !k.trim().is_empty());
        let env = p.env.iter().find(|v| set(v)).or(p.env.first()).cloned();
        if key.is_empty() && !env.iter().any(|v| set(&v)) {
            return Err("no key entered; nothing saved".into());
        }
        let spec = ProviderSpec {
            api_key_env: env,
            keyless: false,
            default_model: model.map(String::from),
            catalog_id: Some(name.into()),
            ..probe::spec(name, kind, &url, key)
        };
        self.add_provider(spec, key)?;
        match model {
            Some(model) => self.switch_model(name, model)?,
            None => self.connected(name),
        }
        Ok(())
    }

    /// Save a provider, and its key if it has one.
    fn add_provider(&mut self, spec: ProviderSpec, key: &str) -> Result<(), String> {
        profile::validate(&spec.name)?;
        let name = spec.name.clone();
        crate::providers::save(&self.app.paths.providers_file(), &spec)?;
        self.app.cfg.providers.insert(name.clone(), spec);
        if !key.is_empty() {
            self.app.keys.set(&name, key);
            self.app.keys.save(&self.app.paths.keys_file())?;
        }
        self.say(&format!(
            "Connected {name} in profile '{}'.",
            self.app.paths.profile
        ));
        Ok(())
    }

    pub(crate) fn open_catalog_search(&mut self) {
        self.open_entry(Entry {
            purpose: Purpose::CatalogSearch,
            title: "Search models.dev".into(),
            hint: "a provider or model name, such as 'deepseek flash' or 'kimi'".into(),
            value: String::new(),
            secret: false,
        });
    }

    /// The search results: each row's value is `provider`, a tab, then `model`.
    fn catalog_search_given(&mut self, query: &str) -> Result<(), String> {
        let cat = catalog::ensure(&self.app.paths.base)?;
        let hits = catalog::search(&cat, query);
        if hits.is_empty() {
            return Err(format!("no model matches '{query}'; try fewer words"));
        }
        let items = hits
            .iter()
            .map(|h| {
                let price = h
                    .cost
                    .and_then(|c| Some(format!("${}/${} per M tokens", c.input?, c.output?)))
                    .unwrap_or_else(|| "price unknown".into());
                PickItem::new(
                    format!("{}  ({})", h.name, h.provider),
                    format!("{}\t{}", h.provider, h.model),
                )
                .detail(price)
            })
            .collect();
        self.open_picker(
            PickKind::CatalogMatch,
            "Models on models.dev · Enter picks one",
            items,
        );
        Ok(())
    }

    /// A catalog model was picked: ask only for its key.
    pub(crate) fn catalog_match_picked(&mut self, value: &str) {
        let Some((provider, model)) = value.split_once('\t') else {
            return;
        };
        // A built-in provider (OpenCode, a coding plan, ...) needs only its key, which it may have.
        if let Ok(spec) = self.app.provider_spec(provider)
            && !self.app.cfg.providers.contains_key(provider)
        {
            if self.app.has_credentials(&spec) {
                if let Err(e) = self.switch_model(provider, model) {
                    self.error(&e);
                }
            } else {
                self.open_key_entry(provider);
            }
            return;
        }
        if self.app.cfg.providers.contains_key(provider) {
            if let Err(e) = self.switch_model(provider, model) {
                self.error(&e);
            }
        } else if let Err(e) = self.start_listed(provider, Some(model)) {
            self.error(&e);
        }
    }

    fn open_entry(&mut self, entry: Entry) {
        self.prompt.completion = None;
        self.prompt.picker = None;
        self.prompt.entry = Some(entry);
        self.dirty = true;
    }

    pub(super) fn on_key_entry(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let Some(e) = self.prompt.entry.as_mut() else {
            return;
        };
        match k.code {
            KeyCode::Esc => self.prompt.entry = None,
            KeyCode::Enter => self.finish_entry(),
            KeyCode::Backspace => {
                e.value.pop();
            }
            KeyCode::Char('u') if ctrl => e.value.clear(),
            KeyCode::Char(c) if !ctrl && !alt => e.value.push(c),
            _ => {}
        }
    }

    /// Paste lands in the box as one line; a newline in a key or a name would be a mistake.
    pub(super) fn paste_into_entry(&mut self, text: &str) {
        if let Some(e) = self.prompt.entry.as_mut() {
            e.value.extend(text.chars().filter(|c| !c.is_control()));
        }
    }

    /// Act on what was typed. On failure the box stays open with the value, so nothing is retyped.
    fn finish_entry(&mut self) {
        let Some(entry) = self.prompt.entry.take() else {
            return;
        };
        let value = entry.value.trim().to_string();
        let result = match &entry.purpose {
            Purpose::ApiKey { provider } => self.save_key(provider, &value),
            Purpose::NewProfile => self.create_profile(&value, None),
            Purpose::ProviderUrl => self.provider_url_given(&value),
            Purpose::ProviderKey { url } => self.provider_key_given(url, &value),
            Purpose::ListedKey { name, model } => self.save_listed(name, &value, model.as_deref()),
            Purpose::ProviderModel { kind, url, key } => {
                self.provider_model_given(*kind, url, key, &value)
            }
            Purpose::CatalogSearch => self.catalog_search_given(&value),
        };
        if let Err(e) = result {
            self.error(&e);
            self.prompt.entry = Some(entry);
        }
        self.dirty = true;
    }

    fn save_key(&mut self, provider: &str, key: &str) -> Result<(), String> {
        if key.is_empty() {
            return Err("no key entered; nothing saved".into());
        }
        self.app.provider_spec(provider)?;
        self.app.keys.set(provider, key);
        self.app.keys.save(&self.app.paths.keys_file())?;
        self.say(&format!(
            "Saved the {provider} key in profile '{}'.",
            self.app.paths.profile
        ));
        self.connected(provider);
        Ok(())
    }

    fn provider_url_given(&mut self, typed: &str) -> Result<(), String> {
        let url = probe::clean_url(typed)?;
        self.open_entry(Entry {
            title: format!("API key for {url}"),
            purpose: Purpose::ProviderKey { url },
            hint: "stored in keys.json, only you can read it; empty for a local server".into(),
            value: String::new(),
            secret: true,
        });
        Ok(())
    }

    fn provider_key_given(&mut self, url: &str, key: &str) -> Result<(), String> {
        if key.is_empty() && !probe::is_local(url) {
            return Err("this server needs a key; type it, or use a local URL".into());
        }
        self.say(&format!("Checking {url}…"));
        let (url, key, tx) = (url.to_string(), key.to_string(), self.tx.clone());
        std::thread::spawn(move || {
            let result = probe::probe(&url, &key);
            let _ = tx.send(Ev::Probed { url, key, result });
        });
        Ok(())
    }

    fn new_provider_name(&self, url: &str) -> String {
        probe::name_for(url, |n| self.app.provider_spec(n).is_ok())
    }

    /// The endpoint answered: save it and choose a model. Otherwise ask what it is.
    pub(super) fn endpoint_probed(
        &mut self,
        url: &str,
        key: &str,
        result: Result<(Kind, String, Vec<ModelInfo>), String>,
    ) {
        match result {
            Ok((kind, base, models)) => {
                let name = self.new_provider_name(&base);
                let spec = probe::spec(&name, kind, &base, key);
                if let Err(e) = self.add_provider(spec.clone(), key) {
                    return self.error(&e);
                }
                let seq = self.models.begin(&spec);
                self.on_models(&name, seq, Ok(models));
                if self.prompt.is_free() {
                    self.open_model_picker_for(&name);
                }
            }
            Err(e) => {
                self.error(&format!("{url}: {e}"));
                if !self.prompt.is_free() {
                    return;
                }
                self.prompt.endpoint = Some((url.into(), key.into()));
                self.open_picker(
                    PickKind::ProviderKind,
                    "Add it anyway? Choose its API style",
                    vec![
                        PickItem::new("OpenAI-compatible", "chat").detail("chat completions"),
                        PickItem::new("Anthropic-compatible", "anthropic").detail("messages API"),
                    ],
                );
            }
        }
    }

    pub(crate) fn provider_kind_picked(&mut self, kind: &str) {
        let Some((url, key)) = self.prompt.endpoint.take() else {
            return;
        };
        let kind = if kind == "anthropic" {
            Kind::Anthropic
        } else {
            Kind::Chat
        };
        self.open_entry(Entry {
            title: format!("Model ID on {url}"),
            purpose: Purpose::ProviderModel { kind, url, key },
            hint: "exactly as the provider names it".into(),
            value: String::new(),
            secret: false,
        });
    }

    fn provider_model_given(
        &mut self,
        kind: Kind,
        url: &str,
        key: &str,
        model: &str,
    ) -> Result<(), String> {
        if model.is_empty() {
            return Err("type the model's ID".into());
        }
        let base = match kind {
            // The Anthropic adapter adds /v1 itself.
            Kind::Anthropic => url.strip_suffix("/v1").unwrap_or(url),
            _ => url,
        };
        let name = self.new_provider_name(base);
        let spec = ProviderSpec {
            default_model: Some(model.into()),
            ..probe::spec(&name, kind, base, key)
        };
        self.add_provider(spec, key)?;
        self.switch_model(&name, model)
    }
}
