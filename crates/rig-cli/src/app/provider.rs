//! Which provider and model to use, and whether there are credentials for them.

use crate::config::default_model;
use rig_core::provider::{Provider, ProviderError};
use rig_provider::{BuildEnv, ProviderSpec};

use super::App;

/// Where a provider's credential comes from, in the order they are tried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// ChatGPT plan: signed in to this profile or not.
    Plan { signed_in: bool },
    /// The server needs none (e.g. a local Ollama).
    Keyless,
    /// `api_key` in a config file.
    Config,
    /// Saved in this profile with `rig key set`.
    Saved,
    /// From this environment variable.
    Env(String),
    /// Nothing found; the environment variable that would have supplied it.
    Missing(String),
}

impl Credential {
    pub fn usable(&self) -> bool {
        !matches!(
            self,
            Credential::Plan { signed_in: false } | Credential::Missing(_)
        )
    }

    pub fn describe(&self) -> String {
        match self {
            Credential::Plan { signed_in: true } => "signed in".into(),
            Credential::Plan { signed_in: false } => "not signed in".into(),
            Credential::Keyless => "no key needed".into(),
            Credential::Config => "key in config".into(),
            Credential::Saved => "key saved in profile".into(),
            Credential::Env(var) => format!("${var}"),
            Credential::Missing(var) => format!("missing ${var}"),
        }
    }
}

impl App {
    pub fn provider_spec(&self, name: &str) -> Result<ProviderSpec, String> {
        let specs = self.cfg.providers();
        specs
            .iter()
            .find(|s| s.name == name)
            .cloned()
            .ok_or_else(|| {
                let known: Vec<_> = specs.iter().map(|s| s.name.as_str()).collect();
                format!("unknown provider '{name}'. Known: {}", known.join(", "))
            })
    }

    pub(super) fn parse_model(&self, m: &str) -> Result<(ProviderSpec, String), String> {
        match m.split_once('/') {
            Some((p, model)) if !model.is_empty() => {
                Ok((self.provider_spec(p)?, model.to_string()))
            }
            _ => {
                let spec = self.provider_spec(m)?;
                let model = default_model(m)
                    .ok_or_else(|| format!("no default model for '{m}'; use {m}/<model>"))?;
                Ok((spec, model.to_string()))
            }
        }
    }

    /// `provider/model` from the flag, then what you used last (if you can still use it), then the
    /// config file, then the first provider that has credentials.
    pub fn resolve_model(&self, flag: Option<&str>) -> Result<(ProviderSpec, String), String> {
        if let Some(m) = flag {
            return self.parse_model(m);
        }
        if let Some(m) = self.state.last(&self.state.model)
            && let Ok((spec, model)) = self.parse_model(m)
            && self.has_credentials(&spec)
        {
            return Ok((spec, model));
        }
        if let Some(m) = self.cfg.model.as_deref() {
            return self.parse_model(m);
        }
        for name in ["chatgpt", "anthropic", "openai", "xai", "openrouter"] {
            let spec = self.provider_spec(name)?;
            if self.has_credentials(&spec) {
                return Ok((spec, default_model(name).unwrap_or_default().to_string()));
            }
        }
        Err("no model is set up. Sign in with `rig login codex`, save a key with `rig key set <provider>`, or export ANTHROPIC_API_KEY / OPENAI_API_KEY / XAI_API_KEY / OPENROUTER_API_KEY".into())
    }

    pub fn credential(&self, spec: &ProviderSpec) -> Credential {
        let var = spec.api_key_env.as_deref().unwrap_or("API key");
        if spec.chatgpt_plan {
            Credential::Plan {
                signed_in: self.paths.chatgpt_auth_file().is_file(),
            }
        } else if spec.api_key.is_some() {
            Credential::Config
        } else if self.keys.get(&spec.name).is_some() {
            Credential::Saved
        } else if spec.keyless && spec.api_key_env.is_none() {
            Credential::Keyless
        } else if spec.api_key_env.as_deref().and_then(Self::getenv).is_some() {
            Credential::Env(var.to_string())
        } else {
            Credential::Missing(var.to_string())
        }
    }

    pub fn has_credentials(&self, spec: &ProviderSpec) -> bool {
        self.credential(spec).usable()
    }

    /// Stands in for a provider that has no credentials yet; every request says how to fix that.
    pub fn unauthenticated(&self, spec: &ProviderSpec, model: &str) -> Box<dyn Provider> {
        Box::new(Unauthenticated {
            name: spec.name.clone(),
            model: model.to_string(),
            hint: sign_in_hint(spec),
        })
    }

    pub fn build_provider(
        &self,
        spec: &ProviderSpec,
        model: &str,
    ) -> Result<Box<dyn Provider>, String> {
        let env = BuildEnv {
            chatgpt_auth_file: self.paths.chatgpt_auth_file(),
            getenv: &Self::getenv,
        };
        // A key saved in the profile stands in for the environment variable.
        let mut spec = spec.clone();
        if spec.api_key.is_none() && !spec.chatgpt_plan {
            spec.api_key = self.keys.get(&spec.name).map(String::from);
        }
        rig_provider::build(&spec, model, &env).map_err(|e| e.to_string())
    }
}

/// What to tell someone who has no credentials for `spec`.
pub fn sign_in_hint(spec: &ProviderSpec) -> String {
    if spec.chatgpt_plan {
        "Not signed in. Type /login to use your ChatGPT plan, or /provider to use an API key."
            .into()
    } else {
        format!(
            "No API key for {0}: type /provider and select it to enter one, run `rig key set {0}`, or set {1}.",
            spec.name,
            spec.api_key_env.as_deref().unwrap_or("its API key")
        )
    }
}

struct Unauthenticated {
    name: String,
    model: String,
    hint: String,
}

impl Provider for Unauthenticated {
    fn name(&self) -> &str {
        &self.name
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn stream(
        &self,
        _: &rig_core::provider::Request,
        _: &mut dyn FnMut(rig_core::provider::StreamEvent),
    ) -> Result<rig_core::provider::Completion, ProviderError> {
        Err(ProviderError::Auth(self.hint.clone()))
    }
}
