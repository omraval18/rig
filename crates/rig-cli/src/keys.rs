//! API keys saved per profile in `keys.json` (mode 0600), keyed by provider name.
//!
//! A saved key beats the provider's environment variable, so two profiles can use two
//! different accounts of the same provider from one shell.

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Keys(BTreeMap<String, String>);

impl Keys {
    pub fn load(path: &Path) -> Keys {
        Keys(
            std::fs::read_to_string(path)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default(),
        )
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if self.0.is_empty() {
            return match std::fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    Err(format!("cannot remove {}: {e}", path.display()))
                }
                _ => Ok(()),
            };
        }
        let text = serde_json::to_string_pretty(&self.0).map_err(|e| e.to_string())?;
        rig_core::fsutil::write_atomic(path, text.as_bytes(), 0o600)
            .map(drop)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    pub fn get(&self, provider: &str) -> Option<&str> {
        self.0
            .get(provider)
            .map(String::as_str)
            .filter(|k| !k.trim().is_empty())
    }

    pub fn set(&mut self, provider: &str, key: &str) {
        self.0.insert(provider.to_string(), key.trim().to_string());
    }

    /// Whether there was a key to remove.
    pub fn remove(&mut self, provider: &str) -> bool {
        self.0.remove(provider).is_some()
    }

    pub fn names(&self) -> Vec<&str> {
        self.0.keys().map(String::as_str).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `sk-ant…7f3a`: enough to tell keys apart, not enough to use one.
pub fn masked(key: &str) -> String {
    let n = key.chars().count();
    if n <= 12 {
        return "•".repeat(n.min(8));
    }
    let head: String = key.chars().take(6).collect();
    let tail: String = key.chars().skip(n - 4).collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_privately_and_an_empty_store_leaves_no_file() {
        let dir = std::env::temp_dir().join(format!("rig-keys-{}", std::process::id()));
        let f = dir.join("keys.json");
        let mut k = Keys::default();
        k.set("anthropic", " sk-ant-123 \n");
        k.save(&f).unwrap();
        let back = Keys::load(&f);
        assert_eq!(back.get("anthropic"), Some("sk-ant-123"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let mut back = back;
        assert!(back.remove("anthropic"));
        back.save(&f).unwrap();
        assert!(!f.exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn masking_hides_the_middle() {
        assert_eq!(masked("sk-ant-api03-abcdefgh7f3a"), "sk-ant…7f3a");
        assert_eq!(masked("short"), "•••••");
    }
}
