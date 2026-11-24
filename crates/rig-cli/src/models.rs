//! The models each connected provider offers. Lists are kept in the profile's `models.json`, so the
//! picker opens at once with what was known last time, and are refreshed in the background.

use rig_core::provider::ModelInfo;
use rig_provider::ProviderSpec;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// How old a list may be before opening the picker fetches it again.
const MAX_AGE: u64 = 10 * 60;

/// One provider's models.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Listing {
    /// The base URL the list came from; a provider pointed elsewhere starts over.
    source: String,
    pub models: Vec<ModelInfo>,
    /// Unix seconds.
    pub fetched_at: u64,
    /// Why the last fetch failed.
    #[serde(skip)]
    pub error: Option<String>,
    /// The fetch in flight. An answer to an older one (another account, say) is dropped.
    #[serde(skip)]
    pending: Option<u64>,
}

impl Listing {
    pub fn loading(&self) -> bool {
        self.pending.is_some()
    }
}

/// What a finished fetch changed.
#[derive(Debug, PartialEq)]
pub struct Update {
    pub total: usize,
    /// Models that were not in the list we had. Empty when we had no list.
    pub added: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Models {
    path: PathBuf,
    listings: BTreeMap<String, Listing>,
    seq: u64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Models {
    pub fn load(path: &Path) -> Self {
        let listings = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self {
            path: path.to_path_buf(),
            listings,
            seq: 0,
        }
    }

    pub fn get(&self, spec: &ProviderSpec) -> Option<&Listing> {
        self.listings
            .get(&spec.name)
            .filter(|l| l.source == spec.base_url)
    }

    /// The models known for `spec`, possibly none.
    pub fn of(&self, spec: &ProviderSpec) -> &[ModelInfo] {
        self.get(spec).map_or(&[], |l| l.models.as_slice())
    }

    pub fn is_stale(&self, spec: &ProviderSpec) -> bool {
        self.get(spec).is_none_or(|l| {
            !l.loading() && (l.error.is_some() || now().saturating_sub(l.fetched_at) > MAX_AGE)
        })
    }

    /// Note that a fetch starts; its answer must carry the returned number.
    pub fn begin(&mut self, spec: &ProviderSpec) -> u64 {
        self.seq += 1;
        if self.get(spec).is_none() {
            self.listings.insert(
                spec.name.clone(),
                Listing {
                    source: spec.base_url.clone(),
                    ..Listing::default()
                },
            );
        }
        if let Some(l) = self.listings.get_mut(&spec.name) {
            l.pending = Some(self.seq);
        }
        self.seq
    }

    /// Store a fetch's answer. `None` when it is not the one being waited for.
    pub fn finish(
        &mut self,
        provider: &str,
        seq: u64,
        result: Result<Vec<ModelInfo>, String>,
    ) -> Option<Result<Update, String>> {
        let l = self.listings.get_mut(provider)?;
        if l.pending != Some(seq) {
            return None;
        }
        l.pending = None;
        match result {
            Ok(models) => {
                let added = models
                    .iter()
                    .filter(|m| !m.hidden && !l.models.is_empty())
                    .filter(|m| !l.models.iter().any(|old| old.id == m.id))
                    .map(|m| m.id.clone())
                    .collect();
                let total = models.iter().filter(|m| !m.hidden).count();
                l.models = models;
                l.fetched_at = now();
                l.error = None;
                self.save();
                Some(Ok(Update { total, added }))
            }
            Err(e) => {
                l.error = Some(e.clone());
                Some(Err(e))
            }
        }
    }

    /// Drop what is known about a provider: its credentials changed or it was removed.
    pub fn forget(&mut self, provider: &str) {
        if self.listings.remove(provider).is_some() {
            self.save();
        }
    }

    fn save(&self) {
        let known: BTreeMap<_, _> = self
            .listings
            .iter()
            .filter(|(_, l)| !l.models.is_empty())
            .collect();
        if let Ok(text) = serde_json::to_string(&known) {
            let _ = rig_core::fsutil::write_atomic(&self.path, text.as_bytes(), 0o600);
        }
    }
}

#[cfg(test)]
mod tests;
