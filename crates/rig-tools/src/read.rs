use crate::util::{bool_arg, resolve, str_arg, usize_arg};
use rig_core::provider::ToolSpec;
use rig_core::text::looks_binary;
use rig_core::tool::{Access, Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};

pub struct ReadFile;

const DEFAULT_LIMIT: usize = 2400;
const MAX_LINE_CHARS: usize = 2000;

impl Tool for ReadFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".into(),
            description: "Read a text file. Requires an absolute path. Returns up to 2400 lines per call; pass start_line (and max_lines) to page through longer files. Set line_numbers=true to get lines as `12→text` when you need to cite or check a line.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute path of the file to read."},
                    "start_line": {"type": "number", "description": "1-based line to start from (default 1)."},
                    "max_lines": {"type": "number", "description": "Maximum lines to return (default 2400)."},
                    "numbered": {"type": "boolean", "description": "Prefix each line with `N→`."}
                },
                "required": ["path"]
            }),
        }
    }

    fn access(&self, _: &Value) -> Access {
        Access::Read
    }

    fn describe(&self, args: &Value) -> String {
        str_arg(args, "path").unwrap_or("?").to_string()
    }

    fn run(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(p) = str_arg(args, "path") else {
            return ToolOutput::err("path is required");
        };
        let path = resolve(&ctx.cwd, p);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => return ToolOutput::err(format!("cannot read {}: {e}", path.display())),
        };
        if looks_binary(&bytes) {
            return ToolOutput::err(format!(
                "{} looks like a binary file (images and PDFs are not supported yet)",
                path.display()
            ));
        }
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.lines().collect();
        let total = lines.len();
        let offset = usize_arg(args, "start_line").unwrap_or(1).max(1);
        let limit = usize_arg(args, "max_lines")
            .unwrap_or(DEFAULT_LIMIT)
            .clamp(1, DEFAULT_LIMIT);
        if total == 0 {
            return ToolOutput::ok("[empty file]");
        }
        if offset > total {
            return ToolOutput::err(format!(
                "start_line {offset} is past the end of the file ({total} lines)"
            ));
        }
        let numbered = bool_arg(args, "numbered");
        let end = (offset - 1 + limit).min(total);
        let mut out = String::new();
        for (i, line) in lines[offset - 1..end].iter().enumerate() {
            if numbered {
                out.push_str(&format!("{}→", offset + i));
            }
            if line.chars().count() > MAX_LINE_CHARS {
                out.extend(line.chars().take(MAX_LINE_CHARS));
                out.push_str("… [line truncated]");
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }
        if end < total {
            out.push_str(&format!(
                "[showing lines {offset}-{end} of {total}; pass start_line={} for more]\n",
                end + 1
            ));
        }
        ToolOutput::ok(out)
    }
}
