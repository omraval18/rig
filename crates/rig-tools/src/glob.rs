use crate::util::{count_summary, resolve, str_arg, str_list};
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use rig_core::provider::ToolSpec;
use rig_core::tool::{Access, Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};

pub struct FindFiles;

const MAX_RESULTS: usize = 1000;

impl Tool for FindFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "find_files".into(),
            description: "Find files by glob pattern (gitignore-style, respects .gitignore). Several patterns are OR-ed; `exclude` removes matches. Examples: \"*.rs\", [\"src/**/*.tsx\"], \"**/*.test.*\".".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "include": {"description": "A glob or an array of globs to include.", "anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]},
                    "exclude": {"description": "A glob or an array of globs to exclude.", "anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]},
                    "root": {"type": "string", "description": "Absolute directory to search (defaults to the working directory)."}
                },
                "required": ["include"]
            }),
        }
    }

    fn access(&self, _: &Value) -> Access {
        Access::Read
    }

    fn describe(&self, args: &Value) -> String {
        str_list(args, "include").join(", ")
    }

    fn summarize(&self, _: &Value, out: &ToolOutput) -> Option<String> {
        count_summary(&out.text, "No files matched.", "file")
    }

    fn run(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let patterns = str_list(args, "include");
        if patterns.is_empty() {
            return ToolOutput::err("patterns is required");
        }
        let root = resolve(&ctx.cwd, str_arg(args, "root").unwrap_or("."));
        let mut ob = OverrideBuilder::new(&root);
        for p in &patterns {
            if let Err(e) = ob.add(p) {
                return ToolOutput::err(format!("bad pattern '{p}': {e}"));
            }
        }
        for p in str_list(args, "exclude") {
            if let Err(e) = ob.add(&format!("!{}", p.trim_start_matches('!'))) {
                return ToolOutput::err(format!("bad exclude pattern '{p}': {e}"));
            }
        }
        let overrides = match ob.build() {
            Ok(o) => o,
            Err(e) => return ToolOutput::err(e.to_string()),
        };
        let mut hits: Vec<(std::time::SystemTime, String)> = Vec::new();
        for entry in WalkBuilder::new(&root)
            .overrides(overrides)
            .build()
            .flatten()
        {
            if entry.file_type().is_some_and(|t| t.is_file()) {
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(std::time::UNIX_EPOCH);
                hits.push((mtime, entry.path().display().to_string()));
            }
        }
        // Newest first: recently touched files are usually the relevant ones.
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        if hits.is_empty() {
            return ToolOutput::ok("No files matched.");
        }
        let total = hits.len();
        let mut out: String = hits
            .iter()
            .take(MAX_RESULTS)
            .map(|h| h.1.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if total > MAX_RESULTS {
            out.push_str(&format!(
                "\n[{} more matches not shown; narrow the pattern]",
                total - MAX_RESULTS
            ));
        }
        ToolOutput::ok(out)
    }
}
