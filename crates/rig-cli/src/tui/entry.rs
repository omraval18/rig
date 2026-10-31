//! The text box over the composer: typing an API key, or naming a new profile.

use super::Tui;
use crate::catalog;
use crate::entry::{Entry, Purpose};
use crate::picker::{PickItem, PickKind};
use crate::profile;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rig_provider::{Kind, ProviderSpec};

/// A base URL on this machine may be plain http and may have no key.
fn is_local(url: &str) -> bool {
    ["http://localhost", "http://127.0.0.1", "http://[::1]"]
        .iter()
        .any(|p| url.starts_with(p))
}

impl Tui {
    /// Ask for a provider's API key. Enter saves it in the active profile and switches to the provider.
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

    /// Start adding a provider: first its name, then the API style, the URL, the key and the model.
    pub(crate) fn open_provider_entry(&mut self) {
        self.open_entry(Entry {
            purpose: Purpose::ProviderName,
            title: "Name for the new provider".into(),
            hint: "letters, digits, '-' and '_'; it is what you will type after the slash".into(),
            value: String::new(),
            secret: false,
        });
    }

    /// The API style was picked; the value is `kind|name`.
    pub(crate) fn provider_kind_picked(&mut self, value: &str) {
        let Some((kind, name)) = value.split_once('|') else {
            return;
        };
        self.open_entry(Entry {
            purpose: Purpose::ProviderUrl {
                name: name.into(),
                kind: kind.into(),
            },
            title: format!("Base URL for {name}"),
            hint: "the API root, such as https://api.example.com/v1 (no /chat/completions)".into(),
            value: String::new(),
            secret: false,
        });
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
        let result = (|| -> Result<(), String> {
            let cat = catalog::ensure(&self.app.paths.base)?;
            let p = cat
                .providers
                .get(provider)
                .ok_or("that provider is no longer listed")?;
            let npm = p.npm.as_deref().unwrap_or("");
            let kind = catalog::kind_for(npm)
                .ok_or_else(|| format!("{provider} uses an API style rig does not speak yet"))?;
            let url =
                catalog::base_url(p, kind).ok_or("the catalog has no URL for this provider")?;
            if self.app.cfg.providers.contains_key(provider) {
                return Err(format!(
                    "'{provider}' is already a provider; pick it from /provider"
                ));
            }
            profile::validate(provider)?;
            self.open_entry(Entry {
                purpose: Purpose::ProviderKey {
                    name: provider.into(),
                    kind: kind.into(),
                    url,
                    model: Some(model.into()),
                    catalog: Some(provider.into()),
                },
                title: format!("API key for {provider} ({model})"),
                hint: "stored in keys.json, only you can read it".into(),
                value: String::new(),
                secret: true,
            });
            Ok(())
        })();
        if let Err(e) = result {
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
            Purpose::NewProfile => self.create_profile(&value),
            Purpose::ProviderName => self.provider_name_given(&value),
            Purpose::ProviderUrl { name, kind } => self.provider_url_given(name, kind, &value),
            Purpose::ProviderKey {
                name,
                kind,
                url,
                model,
                catalog,
            } => match model {
                // Picked from models.dev: the model is already known, so the key saves it.
                Some(model) => {
                    self.save_provider(name, kind, url, &value, model, catalog.as_deref())
                }
                None => self.provider_key_given(name, kind, url, &value),
            },
            Purpose::ProviderModel {
                name,
                kind,
                url,
                key,
            } => self.save_provider(name, kind, url, key, &value, None),
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
        let spec = self.app.provider_spec(provider)?;
        let model = self
            .app
            .default_model_of(&spec)
            .ok_or_else(|| format!("no default model for {provider}; pick one with /model"))?;
        self.app.keys.set(provider, key);
        self.app.keys.save(&self.app.paths.keys_file())?;
        self.switch_model(provider, &model)?;
        self.say(&format!(
            "Saved the {provider} key in profile '{}' and switched to {provider}/{model}.",
            self.app.paths.profile
        ));
        Ok(())
    }

    /// Save a provider the user added, its key, and switch to it with the model they gave.
    fn save_provider(
        &mut self,
        name: &str,
        kind: &str,
        url: &str,
        key: &str,
        model: &str,
        catalog_id: Option<&str>,
    ) -> Result<(), String> {
        profile::validate(name)?;
        let spec = ProviderSpec {
            name: name.into(),
            kind: if kind == "anthropic" {
                Kind::Anthropic
            } else {
                Kind::Chat
            },
            base_url: url.trim_end_matches('/').into(),
            api_key_env: None,
            api_key: None,
            chatgpt_plan: false,
            keyless: key.is_empty(),
            web_search: false,
            default_model: Some(model.into()),
            catalog_id: catalog_id.map(String::from),
        };
        crate::providers::save(&self.app.paths.providers_file(), &spec)?;
        self.app.cfg.providers.insert(name.into(), spec);
        if !key.is_empty() {
            self.app.keys.set(name, key);
            self.app.keys.save(&self.app.paths.keys_file())?;
        }
        self.switch_model(name, model)?;
        self.say(&format!("Added {name} and switched to {name}/{model}."));
        Ok(())
    }

    fn provider_name_given(&mut self, name: &str) -> Result<(), String> {
        profile::validate(name)?;
        if self.app.cfg.providers.contains_key(name) {
            return Err(format!("'{name}' is already a provider; pick another name"));
        }
        self.open_picker(
            PickKind::ProviderKind,
            &format!("API style for {name}"),
            vec![
                PickItem::new("OpenAI-compatible", format!("chat|{name}"))
                    .detail("chat completions, such as DeepSeek, Z.ai, Kimi, Ollama"),
                PickItem::new("Anthropic-compatible", format!("anthropic|{name}"))
                    .detail("messages API, such as MiniMax's Anthropic endpoint"),
            ],
        );
        Ok(())
    }

    fn provider_url_given(&mut self, name: &str, kind: &str, url: &str) -> Result<(), String> {
        let url = url.trim();
        let web = url.starts_with("https://") && url.len() > "https://".len();
        if !(web || (url.starts_with("http://") && is_local(url))) {
            return Err(
                "the base URL must start with https:// (http:// only for this machine)".into(),
            );
        }
        self.open_entry(Entry {
            purpose: Purpose::ProviderKey {
                name: name.into(),
                kind: kind.into(),
                url: url.into(),
                model: None,
                catalog: None,
            },
            title: format!("API key for {name}"),
            hint:
                "stored in keys.json, only you can read it; leave empty for a server without a key"
                    .into(),
            value: String::new(),
            secret: true,
        });
        Ok(())
    }

    fn provider_key_given(
        &mut self,
        name: &str,
        kind: &str,
        url: &str,
        key: &str,
    ) -> Result<(), String> {
        if key.is_empty() && !is_local(url) {
            return Err("this server needs a key; type it, or use a local URL".into());
        }
        self.open_entry(Entry {
            purpose: Purpose::ProviderModel {
                name: name.into(),
                kind: kind.into(),
                url: url.into(),
                key: key.into(),
            },
            title: format!("Model ID for {name}"),
            hint: "exactly as the provider names it, such as gpt-6-sol or deepseek-v4-flash".into(),
            value: String::new(),
            secret: false,
        });
        Ok(())
    }

    fn create_profile(&mut self, name: &str) -> Result<(), String> {
        let base = self.app.paths.base.clone();
        profile::create(&base, name)?;
        // Switching restarts rig in the new profile, where its keys and logins are set up.
        self.cmd_profile(name);
        Ok(())
    }
}
