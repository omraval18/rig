//! Providers the user added from the UI, saved per profile in `providers.json`. The file is written
//! by rig; the key for each one lives in `keys.json`, like the built-in providers' keys.

use rig_provider::ProviderSpec;
use std::collections::BTreeMap;
use std::path::Path;

/// Providers in the file, keyed by name. A missing or unreadable file is no providers.
pub fn load(path: &Path) -> BTreeMap<String, ProviderSpec> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Adds or replaces one provider and rewrites the file atomically.
pub fn save(path: &Path, spec: &ProviderSpec) -> Result<(), String> {
    let mut all = load(path);
    all.insert(spec.name.clone(), spec.clone());
    let text = serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?;
    rig_core::fsutil::write_atomic(path, text.as_bytes(), 0o600)
        .map(drop)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig_provider::Kind;

    #[test]
    fn a_saved_provider_comes_back_with_its_model_and_kind() {
        let dir = std::env::temp_dir().join(format!("rig-providers-{}", std::process::id()));
        let path = dir.join("providers.json");
        let spec = ProviderSpec {
            name: "mine".into(),
            kind: Kind::Chat,
            base_url: "https://llm.example.com/v1".into(),
            api_key_env: None,
            api_key: None,
            chatgpt_plan: false,
            keyless: false,
            web_search: false,
            default_model: Some("big-model".into()),
        };
        save(&path, &spec).unwrap();
        let back = load(&path);
        assert_eq!(
            back.get("mine").map(|s| s.default_model.clone()),
            Some(Some("big-model".into()))
        );
        assert_eq!(back["mine"].kind, Kind::Chat);
        std::fs::remove_dir_all(dir).ok();
    }
}
