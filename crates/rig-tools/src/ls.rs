use crate::util::{resolve, str_arg, str_list};
use globset::{Glob, GlobSetBuilder};
use rig_core::fsutil::list_dir;
use rig_core::provider::ToolSpec;
use rig_core::tool::{Access, Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};

pub struct ListDir;

const MAX_ENTRIES: usize = 500;

impl Tool for ListDir {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_dir".into(),
            description: "List a directory. Directories end with `/`. Prefer find_files and search_text for targeted searches.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute directory path (defaults to the working directory)."},
                    "hide": {"type": "array", "items": {"type": "string"}, "description": "Glob patterns of names to hide, e.g. [\"node_modules\", \"*.log\"]."}
                }
            }),
        }
    }

    fn access(&self, _: &Value) -> Access {
        Access::Read
    }

    fn describe(&self, args: &Value) -> String {
        str_arg(args, "path").unwrap_or(".").to_string()
    }

    fn run(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let dir = resolve(&ctx.cwd, str_arg(args, "path").unwrap_or("."));
        let mut ignore = GlobSetBuilder::new();
        for p in str_list(args, "hide") {
            match Glob::new(p.trim_end_matches("/**")) {
                Ok(g) => {
                    ignore.add(g);
                }
                Err(e) => return ToolOutput::err(format!("bad ignore pattern '{p}': {e}")),
            }
        }
        let ignore = match ignore.build() {
            Ok(s) => s,
            Err(e) => return ToolOutput::err(e.to_string()),
        };
        let mut entries = match list_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => return ToolOutput::err(format!("cannot list {}: {e}", dir.display())),
        };
        entries.retain(|(name, _)| !ignore.is_match(name));
        let total = entries.len();
        let mut out = format!("{}\n", dir.display());
        for (name, is_dir) in entries.iter().take(MAX_ENTRIES) {
            out.push_str(&format!("  {name}{}\n", if *is_dir { "/" } else { "" }));
        }
        if total > MAX_ENTRIES {
            out.push_str(&format!(
                "  [{} more entries not shown]\n",
                total - MAX_ENTRIES
            ));
        }
        ToolOutput::ok(out)
    }
}
