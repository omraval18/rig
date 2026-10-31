use crate::util::{bool_arg, count_summary, resolve, str_arg, usize_arg};
use ignore::overrides::OverrideBuilder;
use ignore::types::TypesBuilder;
use ignore::{WalkBuilder, WalkState};
use regex::RegexBuilder;
use rig_core::provider::ToolSpec;
use rig_core::sync::MutexExt;
use rig_core::text::looks_binary;
use rig_core::tool::{Access, Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Mutex;

pub struct SearchText;

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const DEFAULT_HEAD_LIMIT: usize = 500;

struct FileHits {
    path: PathBuf,
    /// 0-based line indexes that matched.
    matched: Vec<usize>,
    lines: Vec<String>,
}

impl Tool for SearchText {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "search_text".into(),
            description: "Search file contents with a regex (ripgrep semantics, respects .gitignore). output_mode \"file_paths\" (default) lists matching files; \"content\" returns matching lines with optional context.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex (or literal when `literal` is true)."},
                    "path": {"type": "string", "description": "Absolute file or directory to search (defaults to the working directory)."},
                    "glob": {"type": "string", "description": "Only search files matching this glob, e.g. \"*.rs\"."},
                    "mode": {"type": "string", "enum": ["files", "lines"]},
                    "ignore_case": {"type": "boolean"},
                    "file_type": {"type": "string", "description": "File type filter such as \"rust\", \"py\", \"js\"."},
                    "before": {"type": "number"},
                    "after": {"type": "number"},
                    "context": {"type": "number", "description": "Lines of context before and after."},
                    "numbered": {"type": "boolean"},
                    "limit": {"type": "number", "description": "Maximum files or lines to return."},
                    "multiline": {"type": "boolean", "description": "Let `.` match newlines and patterns span lines."},
                    "literal": {"type": "boolean"}
                },
                "required": ["pattern"]
            }),
        }
    }

    fn access(&self, _: &Value) -> Access {
        Access::Read
    }

    fn describe(&self, args: &Value) -> String {
        str_arg(args, "pattern").unwrap_or("?").to_string()
    }

    fn summarize(&self, _: &Value, out: &ToolOutput) -> Option<String> {
        count_summary(&out.text, "No matches found.", "result")
    }

    fn run(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(pattern) = str_arg(args, "pattern") else {
            return ToolOutput::err("pattern is required");
        };
        let re = match build_regex(pattern, args) {
            Ok(re) => re,
            Err(e) => return ToolOutput::err(e),
        };
        let walker = match build_walker(args, ctx) {
            Ok(walker) => walker,
            Err(e) => return ToolOutput::err(e),
        };
        let files = search(walker, &re, bool_arg(args, "multiline"));
        if files.is_empty() {
            return ToolOutput::ok("No matches found.");
        }
        let limit = usize_arg(args, "limit")
            .unwrap_or(DEFAULT_HEAD_LIMIT)
            .max(1);
        if str_arg(args, "mode") == Some("lines") {
            ToolOutput::ok(render_content(&files, args, limit))
        } else {
            ToolOutput::ok(render_paths(&files, limit))
        }
    }
}

fn build_regex(pattern: &str, args: &Value) -> Result<regex::Regex, String> {
    let pattern = if bool_arg(args, "literal") {
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(bool_arg(args, "ignore_case"))
        .multi_line(true)
        .dot_matches_new_line(bool_arg(args, "multiline"))
        .build()
        .map_err(|e| format!("invalid regex: {e}"))
}

/// A gitignore-aware walk of `path`, narrowed by `glob_pattern` and `type` when given.
fn build_walker(args: &Value, ctx: &ToolCtx) -> Result<WalkBuilder, String> {
    let root = resolve(&ctx.cwd, str_arg(args, "path").unwrap_or("."));
    let mut walker = WalkBuilder::new(&root);
    if let Some(glob) = str_arg(args, "glob") {
        let mut overrides = OverrideBuilder::new(if root.is_dir() { &root } else { &ctx.cwd });
        overrides.add(glob).map_err(|e| format!("bad glob: {e}"))?;
        walker.overrides(overrides.build().map_err(|e| e.to_string())?);
    }
    if let Some(kind) = str_arg(args, "file_type") {
        let mut types = TypesBuilder::new();
        types.add_defaults().select(kind);
        let types = types
            .build()
            .map_err(|e| format!("unknown file type '{kind}': {e}"))?;
        walker.types(types);
    }
    Ok(walker)
}

/// Every text file under the walk that matches, sorted by path.
fn search(walker: WalkBuilder, re: &regex::Regex, multiline: bool) -> Vec<FileHits> {
    let results: Mutex<Vec<FileHits>> = Mutex::new(Vec::new());
    walker.build_parallel().run(|| {
        let results = &results;
        Box::new(move |entry| {
            if let Some(hits) = entry.ok().and_then(|e| file_hits(&e, re, multiline)) {
                results.locked().push(hits);
            }
            WalkState::Continue
        })
    });
    let mut files = results
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn file_hits(entry: &ignore::DirEntry, re: &regex::Regex, multiline: bool) -> Option<FileHits> {
    if !entry.file_type().is_some_and(|t| t.is_file()) {
        return None;
    }
    if entry.metadata().map_or(true, |m| m.len() > MAX_FILE_BYTES) {
        return None;
    }
    let bytes = std::fs::read(entry.path()).ok()?;
    if looks_binary(&bytes) {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes);
    let matched = matching_lines(re, &text, multiline);
    if matched.is_empty() {
        return None;
    }
    Some(FileHits {
        path: entry.path().to_path_buf(),
        matched,
        lines: text.lines().map(str::to_string).collect(),
    })
}

/// `mode: "files"`: one path per matching file.
fn render_paths(files: &[FileHits], limit: usize) -> String {
    let mut out: Vec<String> = files
        .iter()
        .take(limit)
        .map(|f| f.path.display().to_string())
        .collect();
    if files.len() > limit {
        out.push(format!("[{} more files not shown]", files.len() - limit));
    }
    out.join("\n")
}

/// `mode: "lines"`: matching lines with context, `path:line:text`.
fn render_content(files: &[FileHits], args: &Value, limit: usize) -> String {
    let context = usize_arg(args, "context");
    let before = usize_arg(args, "before").or(context).unwrap_or(0);
    let after = usize_arg(args, "after").or(context).unwrap_or(0);
    let numbers = args
        .get("numbered")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let mut out: Vec<String> = Vec::new();
    for f in files {
        let mut printed_to: Option<usize> = None;
        for &m in &f.matched {
            let start = m.saturating_sub(before);
            let end = (m + after).min(f.lines.len().saturating_sub(1));
            let from = match printed_to {
                Some(p) if start <= p + 1 => p + 1,
                Some(_) => {
                    out.push("--".into());
                    start
                }
                None => start,
            };
            for i in from..=end {
                if out.len() >= limit {
                    out.push(format!("[output cut at limit={limit}]"));
                    return out.join("\n");
                }
                let sep = if f.matched.binary_search(&i).is_ok() {
                    ':'
                } else {
                    '-'
                };
                let line = f.lines.get(i).map_or("", String::as_str);
                let path = f.path.display();
                out.push(if numbers {
                    format!("{path}{sep}{}{sep}{line}", i + 1)
                } else {
                    format!("{path}{sep}{line}")
                });
            }
            printed_to = Some(printed_to.map_or(end, |p| p.max(end)));
        }
    }
    out.join("\n")
}

/// 0-based, sorted, unique indexes of lines containing a match.
fn matching_lines(re: &regex::Regex, text: &str, multiline: bool) -> Vec<usize> {
    if !multiline {
        return text
            .lines()
            .enumerate()
            .filter(|(_, l)| re.is_match(l))
            .map(|(i, _)| i)
            .collect();
    }
    let mut starts = vec![0usize];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let line_of = |off: usize| starts.partition_point(|&s| s <= off) - 1;
    let mut out: Vec<usize> = Vec::new();
    for m in re.find_iter(text) {
        let (a, b) = (
            line_of(m.start()),
            line_of(m.end().saturating_sub(1).max(m.start())),
        );
        out.extend(a..=b);
    }
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_maps_offsets_to_lines() {
        let re = RegexBuilder::new("b\nc")
            .multi_line(true)
            .dot_matches_new_line(true)
            .build()
            .unwrap();
        assert_eq!(matching_lines(&re, "a\nb\nc\nd\n", true), vec![1, 2]);
    }

    #[test]
    fn single_line_matches() {
        let re = RegexBuilder::new("^fn ").multi_line(true).build().unwrap();
        assert_eq!(
            matching_lines(&re, "fn a()\nlet x\nfn b()\n", false),
            vec![0, 2]
        );
    }
}
