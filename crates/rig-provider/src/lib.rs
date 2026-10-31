//! rig-provider: model backends behind the `rig_core::provider::Provider` trait.
//!
//! Three wire formats cover every backend we support:
//! - [`responses`]: OpenAI Responses API (OpenAI API key, or the ChatGPT plan)
//! - [`anthropic`]: Anthropic Messages API
//! - [`chat`]: OpenAI-compatible Chat Completions (xAI/Grok, OpenRouter, Ollama, ...)

pub mod anthropic;
pub mod auth;
pub mod chat;
pub mod chatgpt;
mod http;
pub mod responses;

use auth::{Auth, StaticHeader};
use http::Endpoint;
use rig_core::provider::{Provider, ProviderError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// OpenAI Responses API.
    Responses,
    /// Anthropic Messages API.
    Anthropic,
    /// OpenAI-compatible Chat Completions.
    Chat,
}

/// A configured backend: where it lives and how it authenticates.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderSpec {
    #[serde(default)]
    pub name: String,
    pub kind: Kind,
    pub base_url: String,
    /// Environment variable holding the API key.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Literal API key. Prefer `api_key_env`.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Authenticate with the user's ChatGPT subscription instead of a key.
    #[serde(default)]
    pub chatgpt_plan: bool,
    /// The server needs no credential (e.g. a local Ollama).
    #[serde(default)]
    pub keyless: bool,
    /// Let the provider run its own web search (a hosted tool). Only the Responses API has one.
    #[serde(default)]
    pub web_search: bool,
    /// The model used when none is named. Set for providers added by the user.
    #[serde(default)]
    pub default_model: Option<String>,
    /// The models.dev provider id this came from, so its prices can be looked up.
    #[serde(default)]
    pub catalog_id: Option<String>,
}

/// Providers available without any configuration.
pub fn builtin_specs() -> Vec<ProviderSpec> {
    let spec = |name: &str, kind, base: &str, env: Option<&str>| ProviderSpec {
        name: name.into(),
        kind,
        base_url: base.into(),
        api_key_env: env.map(Into::into),
        api_key: None,
        chatgpt_plan: false,
        keyless: env.is_none(),
        web_search: false,
        default_model: None,
        catalog_id: None,
    };
    vec![
        ProviderSpec {
            chatgpt_plan: true,
            keyless: false,
            web_search: true,
            ..spec(
                "chatgpt",
                Kind::Responses,
                "https://chatgpt.com/backend-api/codex",
                None,
            )
        },
        ProviderSpec {
            web_search: true,
            ..spec(
                "openai",
                Kind::Responses,
                "https://api.openai.com/v1",
                Some("OPENAI_API_KEY"),
            )
        },
        spec(
            "anthropic",
            Kind::Anthropic,
            "https://api.anthropic.com",
            Some("ANTHROPIC_API_KEY"),
        ),
        spec(
            "xai",
            Kind::Chat,
            "https://api.x.ai/v1",
            Some("XAI_API_KEY"),
        ),
        spec(
            "openrouter",
            Kind::Chat,
            "https://openrouter.ai/api/v1",
            Some("OPENROUTER_API_KEY"),
        ),
        spec("ollama", Kind::Chat, "http://localhost:11434/v1", None),
        // Built-in presets for popular OpenAI- and Anthropic-compatible APIs. Base URLs and key
        // variables come from models.dev; Gemini's OpenAI-compatible endpoint is documented by Google
        // and not yet checked with a live key.
        spec(
            "gemini",
            Kind::Chat,
            "https://generativelanguage.googleapis.com/v1beta/openai",
            Some("GEMINI_API_KEY"),
        ),
        spec(
            "deepseek",
            Kind::Chat,
            "https://api.deepseek.com",
            Some("DEEPSEEK_API_KEY"),
        ),
        // The Anthropic adapter adds /v1/messages itself, so the base stops at /anthropic.
        spec(
            "minimax",
            Kind::Anthropic,
            "https://api.minimax.io/anthropic",
            Some("MINIMAX_API_KEY"),
        ),
        spec(
            "zai",
            Kind::Chat,
            "https://api.z.ai/api/paas/v4",
            Some("ZHIPU_API_KEY"),
        ),
        spec(
            "kimi",
            Kind::Chat,
            "https://api.moonshot.ai/v1",
            Some("MOONSHOT_API_KEY"),
        ),
    ]
}

pub struct BuildEnv<'a> {
    /// Where the ChatGPT-plan tokens live.
    pub chatgpt_auth_file: PathBuf,
    pub getenv: &'a dyn Fn(&str) -> Option<String>,
}

pub fn build(
    spec: &ProviderSpec,
    model: &str,
    env: &BuildEnv,
) -> Result<Box<dyn Provider>, ProviderError> {
    let auth: Box<dyn Auth> = if spec.chatgpt_plan {
        Box::new(chatgpt::ChatGptAuth::new(env.chatgpt_auth_file.clone()))
    } else if spec.keyless && spec.api_key.is_none() && spec.api_key_env.is_none() {
        Box::new(StaticHeader::none())
    } else {
        let key = spec
            .api_key
            .clone()
            .or_else(|| spec.api_key_env.as_deref().and_then(|v| (env.getenv)(v)))
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| {
                let var = spec.api_key_env.as_deref().unwrap_or("an API key");
                ProviderError::Auth(format!("no credentials for '{}': set {var}", spec.name))
            })?;
        match spec.kind {
            Kind::Anthropic => Box::new(StaticHeader::x_api_key(key)),
            _ => Box::new(StaticHeader::bearer(key)),
        }
    };
    let ep = Endpoint::new(
        spec.name.clone(),
        model.to_string(),
        spec.base_url.clone(),
        auth,
    );
    Ok(match spec.kind {
        Kind::Responses => Box::new(responses::Responses::new(
            ep,
            spec.chatgpt_plan,
            spec.web_search,
        )),
        Kind::Anthropic => Box::new(anthropic::Anthropic::new(ep)),
        Kind::Chat => Box::new(chat::ChatCompletions::new(ep)),
    })
}
