//! The text box over the composer: typing an API key, or naming a new profile.

use super::Tui;
use crate::config::default_model;
use crate::entry::{Entry, Purpose};
use crate::profile;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
        let model = default_model(provider)
            .ok_or_else(|| format!("no default model for {provider}; pick one with /model"))?;
        self.app.keys.set(provider, key);
        self.app.keys.save(&self.app.paths.keys_file())?;
        self.switch_model(provider, model)?;
        self.say(&format!(
            "Saved the {provider} key in profile '{}' and switched to {provider}/{model}.",
            self.app.paths.profile
        ));
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
