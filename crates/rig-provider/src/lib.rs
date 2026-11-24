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
    /// Paid by a flat subscription (a coding plan), so turns are not priced.
    #[serde(default)]
    pub subscription: bool,
    /// Models a gateway serves from another API style or URL than `kind` and `base_url`. The first
    /// route that names the model wins; models no route names use `kind` and `base_url`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<Route>,
}

/// Where a gateway sends some of its models.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    /// Model ids. One ending in `*` matches every id that starts with what comes before it.
    pub models: Vec<String>,
    pub kind: Kind,
    pub base_url: String,
}

impl Route {
    pub fn matches(&self, model: &str) -> bool {
        self.models.iter().any(|m| match m.strip_suffix('*') {
            Some(prefix) => model.starts_with(prefix),
            None => m == model,
        })
    }
}

impl ProviderSpec {
    /// The API style and base URL that serve `model`.
    pub fn route(&self, model: &str) -> (Kind, &str) {
        self.routes
            .iter()
            .find(|r| r.matches(model))
            .map_or((self.kind, self.base_url.as_str()), |r| {
                (r.kind, r.base_url.as_str())
            })
    }
}

/// OpenCode's gateways (Zen, pay as you go, and Go, a subscription) take one key for many models
/// and serve each from the endpoint of its own API style. These routes cover the model families
/// whose style is stable; the models.dev catalog, when rig has a copy, decides the rest per model.
fn opencode_routes(root: &str, anthropic: &[&str]) -> Vec<Route> {
    vec![
        Route {
            models: anthropic.iter().map(ToString::to_string).collect(),
            kind: Kind::Anthropic,
            // The Anthropic adapter adds /v1/messages itself.
            base_url: format!("{root}/anthropic"),
        },
        Route {
            models: vec!["gpt-*".into()],
            kind: Kind::Responses,
            base_url: format!("{root}/openai/v1"),
        },
    ]
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
        subscription: false,
        routes: Vec::new(),
    };
    let plan = |name: &str, kind, base: &str, env: &str| ProviderSpec {
        subscription: true,
        ..spec(name, kind, base, Some(env))
    };
    // models.dev has their models and prices, but no base URL.
    let listed = |name: &str, base: &str, env: &str| ProviderSpec {
        catalog_id: Some(name.into()),
        ..spec(name, Kind::Chat, base, Some(env))
    };
    let opencode = |name: &str, root: &str, anthropic: &[&str], subscription| ProviderSpec {
        catalog_id: Some(name.into()),
        subscription,
        routes: opencode_routes(root, anthropic),
        ..spec(
            name,
            Kind::Chat,
            &format!("{root}/openai/v1"),
            Some("OPENCODE_API_KEY"),
        )
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
        listed("groq", "https://api.groq.com/openai/v1", "GROQ_API_KEY"),
        listed("mistral", "https://api.mistral.ai/v1", "MISTRAL_API_KEY"),
        listed("cerebras", "https://api.cerebras.ai/v1", "CEREBRAS_API_KEY"),
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
        // Coding plans: the same vendors billed by subscription. Z.ai and Kimi serve plan keys from
        // their own endpoints; MiniMax takes them on its usual one.
        plan(
            "zai-coding-plan",
            Kind::Chat,
            "https://api.z.ai/api/coding/paas/v4",
            "ZHIPU_API_KEY",
        ),
        plan(
            "minimax-coding-plan",
            Kind::Anthropic,
            "https://api.minimax.io/anthropic",
            "MINIMAX_API_KEY",
        ),
        plan(
            "kimi-coding-plan",
            Kind::Chat,
            "https://api.kimi.ai/coding/v1",
            "KIMI_API_KEY",
        ),
        opencode(
            "opencode",
            "https://opencode.ai/inference",
            &["claude-*"],
            false,
        ),
        opencode(
            "opencode-go",
            "https://opencode.ai/inference/go",
            &["claude-*", "qwen*", "minimax-m2.7", "minimax-m3"],
            true,
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
    let (kind, base_url) = spec.route(model);
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
        match kind {
            Kind::Anthropic => Box::new(StaticHeader::x_api_key(key)),
            _ => Box::new(StaticHeader::bearer(key)),
        }
    };
    let ep = Endpoint::new(
        spec.name.clone(),
        model.to_string(),
        base_url.to_string(),
        auth,
    );
    Ok(match kind {
        Kind::Responses => Box::new(responses::Responses::new(
            ep,
            spec.chatgpt_plan,
            spec.web_search,
        )),
        Kind::Anthropic => Box::new(anthropic::Anthropic::new(ep)),
        Kind::Chat => Box::new(chat::ChatCompletions::new(ep)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin(name: &str) -> ProviderSpec {
        builtin_specs()
            .into_iter()
            .find(|s| s.name == name)
            .unwrap()
    }

    #[test]
    fn opencode_sends_each_model_family_to_its_api() {
        let zen = builtin("opencode");
        assert_eq!(
            zen.route("claude-sonnet-5-5"),
            (Kind::Anthropic, "https://opencode.ai/inference/anthropic")
        );
        assert_eq!(
            zen.route("gpt-6-sol"),
            (Kind::Responses, "https://opencode.ai/inference/openai/v1")
        );
        assert_eq!(
            zen.route("glm-5.3"),
            (Kind::Chat, "https://opencode.ai/inference/openai/v1")
        );
        let go = builtin("opencode-go");
        assert!(go.subscription);
        assert_eq!(go.route("minimax-m3").0, Kind::Anthropic);
        assert_eq!(go.route("minimax-m3.1-flash").0, Kind::Chat);
        assert_eq!(
            go.route("kimi-k3"),
            (Kind::Chat, "https://opencode.ai/inference/go/openai/v1")
        );
    }

    #[test]
    fn a_routed_model_builds_with_its_own_api() {
        let env = BuildEnv {
            chatgpt_auth_file: PathBuf::new(),
            getenv: &|v| (v == "OPENCODE_API_KEY").then(|| "k".to_string()),
        };
        let go = builtin("opencode-go");
        for model in ["claude-haiku-5-5", "gpt-6-luna", "glm-5.3"] {
            let p = build(&go, model, &env).unwrap();
            assert_eq!((p.name(), p.model()), ("opencode-go", model));
        }
    }

    #[test]
    fn routes_survive_a_round_trip_and_are_left_out_when_empty() {
        let zen = builtin("opencode");
        let back: ProviderSpec =
            serde_json::from_str(&serde_json::to_string(&zen).unwrap()).unwrap();
        assert_eq!(back.routes, zen.routes);
        let plain = serde_json::to_string(&builtin("deepseek")).unwrap();
        assert!(!plain.contains("routes"));
    }
}
