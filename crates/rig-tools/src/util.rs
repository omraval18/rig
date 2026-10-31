//! Small helpers shared by tools.

use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

pub fn bool_arg(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

pub fn usize_arg(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0) as u64)))
        .map(|n| n as usize)
}

/// Accepts either a single string or an array of strings.
pub fn str_list(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// Tools ask for absolute paths; resolve a relative one against the session cwd rather than failing.
pub fn resolve(cwd: &Path, p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// "No matches" or "N results" for a listing tool; the trailing `[... more not shown]` note is not a result.
pub fn count_summary(text: &str, none: &str, noun: &str) -> Option<String> {
    if text.trim() == none {
        return Some(none.trim_end_matches('.').to_lowercase());
    }
    let n = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('['))
        .count();
    Some(rig_core::text::count(n, noun))
}

#[cfg(test)]
mod count_tests {
    use super::count_summary;

    #[test]
    fn counts_results_and_ignores_the_overflow_note() {
        assert_eq!(
            count_summary("No files matched.", "No files matched.", "file").as_deref(),
            Some("no files matched")
        );
        assert_eq!(
            count_summary("a\nb\n[3 more matches not shown]", "x", "file").as_deref(),
            Some("2 files")
        );
        assert_eq!(
            count_summary("a", "x", "result").as_deref(),
            Some("1 result")
        );
    }
}
