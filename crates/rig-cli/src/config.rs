//! Configuration: `~/.rig/config.toml`, then the active profile's `config.toml`, then the
//! project's `.rig/config.toml`.

use rig_provider::{ProviderSpec, builtin_specs};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// `provider/model`, e.g. `chatgpt/gpt-6-sol` or `anthropic/claude-sonnet-5-5`.
    pub model: Option<String>,
    pub effort: Option<String>,
    pub autonomy: Option<String>,
    pub max_turns: Option<usize>,
    /// Override the model's context window (tokens) used for the context meter and auto-compaction.
    pub context_window: Option<u64>,
    /// Compact when the request reaches this percent of the window (10 to 80, default 80).
    pub auto_compact_percent: Option<u8>,
    /// Extra or overriding providers, keyed by name.
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderSpec>,
    /// MCP servers, keyed by name (`[mcp.<name>]`).
    #[serde(default)]
    pub mcp: BTreeMap<String, rig_mcp::ServerSpec>,
}

impl Config {
    /// Later files win for scalars; provider tables merge by name.
    pub fn load(files: &[PathBuf]) -> Result<Config, String> {
        let mut out = Config::default();
        for f in files {
            let Ok(text) = std::fs::read_to_string(f) else {
                continue;
            };
            let c: Config = toml::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))?;
            out.model = c.model.or(out.model);
            out.effort = c.effort.or(out.effort);
            out.autonomy = c.autonomy.or(out.autonomy);
            out.max_turns = c.max_turns.or(out.max_turns);
            out.context_window = c.context_window.or(out.context_window);
            out.auto_compact_percent = c.auto_compact_percent.or(out.auto_compact_percent);
            out.providers.extend(c.providers);
            out.mcp.extend(c.mcp);
        }
        Ok(out)
    }

    /// Built-in providers with config entries layered on top.
    pub fn providers(&self) -> Vec<ProviderSpec> {
        let mut all: BTreeMap<String, ProviderSpec> = builtin_specs()
            .into_iter()
            .map(|s| (s.name.clone(), s))
            .collect();
        for (name, spec) in &self.providers {
            all.insert(
                name.clone(),
                ProviderSpec {
                    name: name.clone(),
                    ..spec.clone()
                },
            );
        }
        all.into_values().collect()
    }
}

pub struct Paths {
    /// `~/.rig` (or `$RIG_HOME`): shared by every profile.
    pub base: PathBuf,
    /// The active profile's own directory: `base` itself for the default profile,
    /// `base/profiles/<name>` for the others. Logins, keys, settings, history and sessions live here.
    pub root: PathBuf,
    /// Name of the active profile.
    pub profile: String,
    pub home: Option<PathBuf>,
}

impl Paths {
    /// Pick the profile from `flag`, then `$RIG_PROFILE`, then the one last chosen with
    /// `rig profile use`. A profile named explicitly must exist; a stale saved choice falls back
    /// to the default.
    pub fn discover(flag: Option<&str>) -> Result<Paths, String> {
        let (base, home) = Paths::base();
        let profile = match Paths::named_profile(flag) {
            Some(name) => {
                crate::profile::check_exists(&base, &name)?;
                name
            }
            None => crate::profile::active(&base),
        };
        Ok(Paths::for_profile(base, profile, home))
    }

    /// `~/.rig` (or `$RIG_HOME`) and `$HOME`.
    pub fn base() -> (PathBuf, Option<PathBuf>) {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let base = std::env::var_os("RIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".rig")))
            .unwrap_or_else(|| PathBuf::from(".rig"));
        (base, home)
    }

    /// The profile asked for by `--profile` or `$RIG_PROFILE`, if any.
    pub fn named_profile(flag: Option<&str>) -> Option<String> {
        flag.map(String::from)
            .or_else(|| std::env::var("RIG_PROFILE").ok())
            .filter(|n| !n.is_empty())
    }

    pub fn for_profile(base: PathBuf, profile: String, home: Option<PathBuf>) -> Paths {
        let root = crate::profile::dir(&base, &profile);
        Paths {
            base,
            root,
            profile,
            home,
        }
    }

    pub fn is_default_profile(&self) -> bool {
        self.profile == crate::profile::DEFAULT
    }

    pub fn chatgpt_auth_file(&self) -> PathBuf {
        std::env::var_os("RIG_CHATGPT_AUTH_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.root.join("auth.json"))
    }

    /// API keys saved with `rig key set`, for this profile only.
    pub fn keys_file(&self) -> PathBuf {
        self.root.join("keys.json")
    }

    /// Providers the user added from the UI, for this profile (`providers.json`).
    pub fn providers_file(&self) -> PathBuf {
        self.root.join("providers.json")
    }

    pub fn history_file(&self) -> PathBuf {
        self.root.join("history")
    }

    pub fn state_file(&self) -> PathBuf {
        self.root.join("state.json")
    }

    pub fn allowlist_file(&self) -> PathBuf {
        self.root.join("allowlist.json")
    }

    /// Shared config, then the profile's own (named profiles only), then the project's. Later wins.
    pub fn config_files(&self, cwd: &Path) -> Vec<PathBuf> {
        let mut files = vec![self.base.join("config.toml")];
        if self.root != self.base {
            files.push(self.root.join("config.toml"));
        }
        files.push(cwd.join(".rig").join("config.toml"));
        files
    }
}

/// Context window to assume when the provider does not report one.
pub fn default_context_window(provider: &str) -> u64 {
    match provider {
        "chatgpt" => 272_000,
        "openai" => 400_000,
        "anthropic" => 200_000,
        _ => 128_000,
    }
}

/// Reasonable default model per built-in provider; override with `--model` or config.
pub fn default_model(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "chatgpt" | "openai" => "gpt-6-sol",
        "anthropic" => "claude-sonnet-5-5",
        "xai" => "grok-4.7",
        "openrouter" => "anthropic/claude-sonnet-5-5",
        "ollama" => "qwen3-coder",
        "gemini" => "gemini-3.8-flash",
        "deepseek" => "deepseek-v4-flash",
        "minimax" => "MiniMax-M3",
        "zai" => "glm-5.3",
        "kimi" => "kimi-k3",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_config_overrides_user_and_merges_providers() {
        let dir = std::env::temp_dir().join(format!("rig-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.toml"), dir.join("b.toml"));
        std::fs::write(&a, "model = \"openai/x\"\nautonomy = \"low\"\n[providers.mine]\nkind = \"chat\"\nbase_url = \"http://h/v1\"\nkeyless = true\n").unwrap();
        std::fs::write(&b, "model = \"anthropic/y\"\n").unwrap();
        let c = Config::load(&[a, b]).unwrap();
        assert_eq!(c.model.as_deref(), Some("anthropic/y"));
        assert_eq!(c.autonomy.as_deref(), Some("low"));
        let names: Vec<_> = c.providers().into_iter().map(|p| p.name).collect();
        assert!(names.contains(&"mine".to_string()) && names.contains(&"chatgpt".to_string()));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(toml::from_str::<Config>("modle = \"x\"").is_err());
    }
}
