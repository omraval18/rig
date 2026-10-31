//! Workspace file index behind `@` completion.
//!
//! A background thread walks the workspace once (honoring `.gitignore`), so the
//! first keystroke after `@` never waits on the filesystem. Queries that leave the
//! workspace (`@~`, `@..`, `@/etc/`) browse the real directory instead.

use ignore::WalkBuilder;
use rig_core::fuzzy::{rank_label, rank_path};
use rig_core::sync::RwLockExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

const MAX_ENTRIES: usize = 150_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Text to insert after `@` (relative path, or the typed base for browsed folders). Folders end with `/`.
    pub path: String,
    pub is_dir: bool,
    pub score: i32,
    /// Char positions in `path` that matched, for highlighting.
    pub positions: Vec<usize>,
}

#[derive(Default)]
struct State {
    /// Relative paths; directories end with `/`. Boxed, with no lowercase copy (matching folds case
    /// as it goes), so a large workspace costs one tight allocation per path.
    entries: Vec<Box<str>>,
    ready: bool,
    truncated: bool,
}

#[derive(Clone)]
pub struct Index {
    root: PathBuf,
    home: Option<PathBuf>,
    state: Arc<RwLock<State>>,
}

impl Index {
    /// Start indexing `root` in the background.
    pub fn spawn(root: PathBuf, home: Option<PathBuf>) -> Index {
        let idx = Index {
            root,
            home,
            state: Arc::default(),
        };
        idx.refresh();
        idx
    }

    pub fn refresh(&self) {
        let (root, state) = (self.root.clone(), self.state.clone());
        state.write_locked().ready = false;
        std::thread::spawn(move || {
            let mut entries = Vec::new();
            let mut truncated = false;
            let walker = WalkBuilder::new(&root)
                .hidden(false)
                .filter_entry(|e| e.file_name() != ".git")
                .build();
            for e in walker.flatten() {
                if e.depth() == 0 {
                    continue;
                }
                let Ok(rel) = e.path().strip_prefix(&root) else {
                    continue;
                };
                let mut p = rel.to_string_lossy().replace('\\', "/");
                if e.file_type().is_some_and(|t| t.is_dir()) {
                    p.push('/');
                }
                entries.push(p.into_boxed_str());
                if entries.len() >= MAX_ENTRIES {
                    truncated = true;
                    break;
                }
            }
            entries.shrink_to_fit();
            let mut s = state.write_locked();
            *s = State {
                entries,
                ready: true,
                truncated,
            };
        });
    }

    pub fn ready(&self) -> bool {
        self.state.read_locked().ready
    }

    pub fn len(&self) -> usize {
        self.state.read_locked().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn truncated(&self) -> bool {
        self.state.read_locked().truncated
    }

    /// Candidates for the text after `@`.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        if let Some(hits) = self.browse(query, limit) {
            return hits;
        }
        let st = self.state.read_locked();
        if query.is_empty() {
            let mut top: Vec<Hit> = st
                .entries
                .iter()
                .filter(|p| p.trim_end_matches('/').matches('/').count() == 0)
                .map(|p| Hit {
                    path: p.to_string(),
                    is_dir: p.ends_with('/'),
                    score: 0,
                    positions: vec![],
                })
                .collect();
            top.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.path.cmp(&b.path)));
            top.truncate(limit);
            return top;
        }
        let q = query.to_lowercase();
        let mut scored: Vec<Hit> = st
            .entries
            .iter()
            // "docs/" means "what is inside docs", so the folder itself is not a candidate.
            .filter(|p| !(query.ends_with('/') && &***p == query))
            .filter(|p| is_subsequence(&q, p))
            .filter_map(|p| {
                rank_path(query, p).map(|m| Hit {
                    path: p.to_string(),
                    is_dir: p.ends_with('/'),
                    score: m.score,
                    positions: m.positions,
                })
            })
            .collect();
        scored.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.path.len().cmp(&b.path.len()))
                .then_with(|| a.path.cmp(&b.path))
        });
        scored.truncate(limit);
        scored
    }

    /// `@~`, `@.`, `@..`, `@/abs/`, `@../x`: list the real directory, filtered by the last segment.
    fn browse(&self, query: &str, limit: usize) -> Option<Vec<Hit>> {
        let leaves_workspace = query == "~"
            || query == "."
            || query == ".."
            || query.starts_with("~/")
            || query.starts_with('/')
            || query.starts_with("../")
            || query.starts_with("./");
        if !leaves_workspace {
            return None;
        }
        let (base, name) = match query {
            "~" | "." | ".." => (format!("{query}/"), String::new()),
            _ => match query.rfind('/') {
                Some(i) => (query[..=i].to_string(), query[i + 1..].to_string()),
                None => (String::new(), query.to_string()),
            },
        };
        let dir = rig_core::expand::resolve_path(
            base.trim_end_matches('/').trim_end_matches('/'),
            &self.root,
            self.home.as_deref(),
        );
        let dir = if base == "/" { PathBuf::from("/") } else { dir };
        let rd = std::fs::read_dir(&dir).ok()?;
        let mut hits: Vec<Hit> = rd
            .flatten()
            .filter_map(|e| {
                let fname = e.file_name().to_string_lossy().into_owned();
                let is_dir = e.path().is_dir();
                if fname == ".git" || (fname.starts_with('.') && !name.starts_with('.')) {
                    return None;
                }
                let m = rank_label(&name, &fname)?;
                let skip = base.chars().count();
                Some(Hit {
                    path: format!("{base}{fname}{}", if is_dir { "/" } else { "" }),
                    is_dir,
                    score: m.score,
                    positions: m.positions.into_iter().map(|p| p + skip).collect(),
                })
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| b.is_dir.cmp(&a.is_dir))
                .then_with(|| a.path.cmp(&b.path))
        });
        hits.truncate(limit);
        Some(hits)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Whether lowercase `q` appears in order in `hay`, ignoring case.
fn is_subsequence(q: &str, hay: &str) -> bool {
    let mut it = hay.chars().flat_map(char::to_lowercase);
    q.chars().all(|c| it.any(|h| h == c))
}

#[cfg(test)]
mod tests;
