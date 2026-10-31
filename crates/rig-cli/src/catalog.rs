//! models.dev: the public catalog of providers and models, with their prices. A trimmed copy lives in
//! `<rig home>/catalog.json`, shared by all profiles. It is fetched when first needed, refreshed after
//! a week, and on `/refresh-models`. Nothing from it is compiled into the binary.

use rig_core::pricing::Price;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Where the catalog comes from. `RIG_CATALOG_URL` replaces it, for tests and mirrors.
const DEFAULT_URL: &str = "https://models.dev/api.json?type=all";
const MAX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_AGE: u64 = 7 * 24 * 3600;
/// Most matches a search returns.
const LIMIT: usize = 40;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Cost {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Model {
    pub name: String,
    pub cost: Option<Cost>,
    pub context: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Provider {
    pub name: String,
    /// The API root, as models.dev lists it. Anthropic-style entries may end in `/v1`.
    pub api: Option<String>,
    /// The SDK package, which says which API style the provider speaks.
    pub npm: Option<String>,
    pub env: Vec<String>,
    pub models: BTreeMap<String, Model>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    /// Unix seconds of the download.
    pub fetched_at: u64,
    pub providers: BTreeMap<String, Provider>,
}

// The download is read straight into these, so the full file is never held as a generic tree.
#[derive(Deserialize)]
struct RawProvider {
    #[serde(default)]
    name: String,
    api: Option<String>,
    npm: Option<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    models: BTreeMap<String, RawModel>,
}

#[derive(Deserialize)]
struct RawModel {
    #[serde(default)]
    name: String,
    cost: Option<Cost>,
    limit: Option<RawLimit>,
}

#[derive(Deserialize)]
struct RawLimit {
    context: Option<u64>,
}

/// One search result: a model, and the provider that serves it.
#[derive(Debug, PartialEq)]
pub struct Match {
    pub provider: String,
    pub model: String,
    pub name: String,
    pub cost: Option<Cost>,
}

pub fn path(base: &Path) -> PathBuf {
    base.join("catalog.json")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The saved copy, however old. `None` when there is none yet.
pub fn load(base: &Path) -> Option<Catalog> {
    let text = std::fs::read_to_string(path(base)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Whether the saved copy is recent enough to use without downloading.
pub fn is_fresh(c: &Catalog) -> bool {
    now().saturating_sub(c.fetched_at) < MAX_AGE
}

/// Download the catalog now and save the trimmed copy.
pub fn refresh(base: &Path) -> Result<Catalog, String> {
    let url = std::env::var("RIG_CATALOG_URL").unwrap_or_else(|_| DEFAULT_URL.into());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent(concat!("rig/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut resp = agent
        .get(&url)
        .call()
        .map_err(|e| format!("could not download the model list: {e}"))?;
    let reader = resp.body_mut().with_config().limit(MAX_BYTES).reader();
    let raw: BTreeMap<String, RawProvider> = serde_json::from_reader(reader)
        .map_err(|e| format!("the model list is not readable: {e}"))?;
    let cat = trim(raw, now());
    let text = serde_json::to_string(&cat).map_err(|e| e.to_string())?;
    rig_core::fsutil::write_atomic(&path(base), text.as_bytes(), 0o644)
        .map_err(|e| format!("cannot save the model list: {e}"))?;
    Ok(cat)
}

fn trim(raw: BTreeMap<String, RawProvider>, fetched_at: u64) -> Catalog {
    let providers = raw
        .into_iter()
        .map(|(id, p)| {
            let models = p
                .models
                .into_iter()
                .map(|(mid, m)| {
                    let model = Model {
                        name: m.name,
                        cost: m.cost,
                        context: m.limit.and_then(|l| l.context),
                    };
                    (mid, model)
                })
                .collect();
            let provider = Provider {
                name: p.name,
                api: p.api,
                npm: p.npm,
                env: p.env,
                models,
            };
            (id, provider)
        })
        .collect();
    Catalog {
        fetched_at,
        providers,
    }
}

/// The catalog to use now: the saved copy if it is fresh, otherwise a download. If the download fails,
/// an older saved copy still beats nothing.
pub fn ensure(base: &Path) -> Result<Catalog, String> {
    let saved = load(base);
    if let Some(c) = saved.as_ref().filter(|c| is_fresh(c)) {
        return Ok(c.clone());
    }
    match refresh(base) {
        Ok(c) => Ok(c),
        Err(e) => saved.ok_or(e),
    }
}

/// The API style for an SDK package: `anthropic`, `chat`, or `None` when rig cannot speak it yet.
pub fn kind_for(npm: &str) -> Option<&'static str> {
    match npm {
        "@ai-sdk/anthropic" => Some("anthropic"),
        "@ai-sdk/openai" | "@ai-sdk/openai-compatible" => Some("chat"),
        _ => None,
    }
}

/// The base URL rig should use for a provider of `kind`. The Anthropic adapter adds `/v1/messages`
/// itself, so a `/v1` at the end of the catalog's entry is dropped for it.
pub fn base_url(provider: &Provider, kind: &str) -> Option<String> {
    let api = provider.api.as_deref()?.trim_end_matches('/');
    Some(if kind == "anthropic" {
        api.strip_suffix("/v1").unwrap_or(api).to_string()
    } else {
        api.to_string()
    })
}

/// Models whose id, name, provider id or provider name contain every word of `query`. A word that
/// names the provider counts for more than one found only in the model, so `moonshot kimi` puts
/// Moonshot's own Kimi models before other providers that resell them.
pub fn search(c: &Catalog, query: &str) -> Vec<Match> {
    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, Match)> = Vec::new();
    for (pid, p) in &c.providers {
        let provider_hay = format!("{pid} {}", p.name).to_lowercase();
        for (mid, m) in &p.models {
            let model_hay = format!("{mid} {}", m.name).to_lowercase();
            if !words
                .iter()
                .all(|w| provider_hay.contains(w.as_str()) || model_hay.contains(w.as_str()))
            {
                continue;
            }
            let score = words
                .iter()
                .map(|w| {
                    if provider_hay.contains(w.as_str()) {
                        2
                    } else {
                        1
                    }
                })
                .sum();
            scored.push((
                score,
                Match {
                    provider: pid.clone(),
                    model: mid.clone(),
                    name: m.name.clone(),
                    cost: m.cost,
                },
            ));
        }
    }
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.provider.cmp(&b.1.provider))
            .then_with(|| a.1.model.cmp(&b.1.model))
    });
    scored.into_iter().take(LIMIT).map(|(_, m)| m).collect()
}

/// The list price of a model from the catalog, for pricing turns on providers added from it.
pub fn price(c: &Catalog, provider: &str, model: &str) -> Option<Price> {
    let cost = c.providers.get(provider)?.models.get(model)?.cost?;
    Some(Price {
        input: cost.input?,
        output: cost.output?,
        cache_read: cost.cache_read.unwrap_or(0.0),
        cache_write: cost.cache_write.unwrap_or(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Catalog {
        let raw = r#"{
            "minimax": {"name": "MiniMax", "api": "https://api.minimax.io/anthropic/v1",
                "npm": "@ai-sdk/anthropic", "env": ["MINIMAX_API_KEY"],
                "models": {"MiniMax-M3": {"name": "MiniMax-M3", "cost": {"input": 0.3, "output": 1.2, "cache_read": 0.03},
                                          "limit": {"context": 1000000, "output": 524288}, "tool_call": true}}},
            "deepseek": {"name": "DeepSeek", "api": "https://api.deepseek.com", "npm": "@ai-sdk/openai-compatible",
                "env": ["DEEPSEEK_API_KEY"],
                "models": {"deepseek-v4-flash": {"name": "DeepSeek V4 Flash", "cost": {"input": 0.15, "output": 0.6}},
                           "free-model": {"name": "Free"}}}
        }"#;
        let parsed: BTreeMap<String, RawProvider> = serde_json::from_str(raw).unwrap();
        trim(parsed, 1000)
    }

    #[test]
    fn the_download_is_trimmed_to_what_rig_uses() {
        let c = fixture();
        let mm = &c.providers["minimax"];
        assert_eq!(mm.env, ["MINIMAX_API_KEY"]);
        assert_eq!(mm.models["MiniMax-M3"].context, Some(1_000_000));
        assert_eq!(mm.models["MiniMax-M3"].cost.unwrap().input, Some(0.3));
    }

    #[test]
    fn sdk_packages_map_to_our_api_styles_and_anthropic_urls_lose_v1() {
        assert_eq!(kind_for("@ai-sdk/anthropic"), Some("anthropic"));
        assert_eq!(kind_for("@ai-sdk/openai-compatible"), Some("chat"));
        assert_eq!(kind_for("@ai-sdk/google"), None);
        let c = fixture();
        assert_eq!(
            base_url(&c.providers["minimax"], "anthropic").as_deref(),
            Some("https://api.minimax.io/anthropic")
        );
        assert_eq!(
            base_url(&c.providers["deepseek"], "chat").as_deref(),
            Some("https://api.deepseek.com")
        );
    }

    #[test]
    fn search_matches_every_word_and_returns_prices() {
        let c = fixture();
        let hits = search(&c, "flash");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].provider, "deepseek");
        assert_eq!(hits[0].model, "deepseek-v4-flash");
        assert!(search(&c, "minimax deepseek").is_empty());
        assert!(search(&c, "   ").is_empty());
        let p = price(&c, "deepseek", "deepseek-v4-flash").unwrap();
        assert_eq!((p.input, p.output), (0.15, 0.6));
        assert!(price(&c, "deepseek", "free-model").is_none());
    }

    #[test]
    fn the_provider_named_in_the_query_comes_first() {
        let mut c = fixture();
        // A reseller that lists the same model: it matches the words only through the model.
        c.providers.insert(
            "aaa-reseller".into(),
            Provider {
                name: "Reseller".into(),
                models: BTreeMap::from([(
                    "deepseek-v4-flash".into(),
                    Model {
                        name: "DeepSeek V4 Flash".into(),
                        ..Model::default()
                    },
                )]),
                ..Provider::default()
            },
        );
        let hits = search(&c, "deepseek flash");
        assert_eq!(
            hits[0].provider, "deepseek",
            "the named provider ranks first"
        );
        assert_eq!(hits[1].provider, "aaa-reseller");
    }

    #[test]
    fn a_fresh_copy_is_used_and_an_old_one_is_not() {
        let mut c = fixture();
        c.fetched_at = now();
        assert!(is_fresh(&c));
        c.fetched_at = now() - MAX_AGE - 1;
        assert!(!is_fresh(&c));
    }
}
