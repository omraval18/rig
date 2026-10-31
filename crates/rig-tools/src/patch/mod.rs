//! `apply_patch` format: the "*** Begin Patch" envelope used by Codex-family
//! models. All operations are planned in memory first; nothing is written
//! unless every hunk applies.

use crate::util::{resolve, str_arg};
use rig_core::provider::ToolSpec;
use rig_core::sync::MutexExt;
use rig_core::tool::{Access, Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

mod apply;
mod parse;

use apply::apply_hunks;
use parse::{Hunk, Op, parse};

pub struct EditFiles;

const FORMAT_HELP: &str = "\
Create, edit, delete or move files with one patch. Pass the full patch text in `patch`:

*** Begin Patch
*** Add File: /abs/path/new.txt
+first line
+second line
*** Update File: /abs/path/existing.rs
*** Move to: /abs/path/renamed.rs      (optional)
@@ fn surrounding_function()           (optional anchor line)
 unchanged context line
-line to remove
+line to add
 unchanged context line
*** Delete File: /abs/path/old.txt
*** End Patch

Context lines start with a space, removals with `-`, additions with `+`. Show about 3 lines of context around each change, enough to be unique in the file. Paths must be absolute.";

impl Tool for EditFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "edit_files".into(),
            description: FORMAT_HELP.into(),
            parameters: json!({
                "type": "object",
                "properties": {"patch": {"type": "string", "description": "The entire patch, from `*** Begin Patch` to `*** End Patch`."}},
                "required": ["patch"]
            }),
        }
    }

    fn access(&self, _: &Value) -> Access {
        Access::Write
    }

    fn describe(&self, args: &Value) -> String {
        match parse(str_arg(args, "patch").unwrap_or("")) {
            Ok(ops) => ops
                .iter()
                .map(|o| match o {
                    Op::Add { path, content } => {
                        format!("add {path} (+{})", content.lines().count())
                    }
                    Op::Delete { path } => format!("delete {path}"),
                    Op::Update { path, hunks, .. } => {
                        let count = |c: char| {
                            hunks
                                .iter()
                                .flat_map(|h| &h.lines)
                                .filter(|(k, _)| *k == c)
                                .count()
                        };
                        format!("edit {path} (+{} -{})", count('+'), count('-'))
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
            Err(_) => "invalid patch".into(),
        }
    }

    fn run(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let ops = match parse(str_arg(args, "patch").unwrap_or("")) {
            Ok(o) => o,
            Err(e) => return ToolOutput::err(format!("invalid patch: {e}")),
        };
        // Plan: (path, Some(new contents) | None = delete), plus a report line.
        let mut writes: Vec<(PathBuf, Option<String>)> = Vec::new();
        let mut report = Vec::new();
        for op in &ops {
            match op {
                Op::Add { path, content } => {
                    let p = resolve(&ctx.cwd, path);
                    writes.push((p, Some(content.clone())));
                    report.push(format!("A {path}"));
                }
                Op::Delete { path } => {
                    let p = resolve(&ctx.cwd, path);
                    if !p.is_file() {
                        return ToolOutput::err(format!("cannot delete {path}: no such file"));
                    }
                    writes.push((p, None));
                    report.push(format!("D {path}"));
                }
                Op::Update {
                    path,
                    move_to,
                    hunks,
                } => {
                    let p = resolve(&ctx.cwd, path);
                    let orig = match fs::read_to_string(&p) {
                        Ok(s) => s,
                        Err(e) => return ToolOutput::err(format!("cannot read {path}: {e}")),
                    };
                    let updated = match apply_hunks(&orig, hunks) {
                        Ok(s) => s,
                        Err(e) => return ToolOutput::err(format!("{path}: {e}")),
                    };
                    match move_to {
                        Some(dest) => {
                            writes.push((p, None));
                            writes.push((resolve(&ctx.cwd, dest), Some(updated)));
                            report.push(format!("R {path} -> {dest}"));
                        }
                        None => {
                            writes.push((p, Some(updated)));
                            report.push(format!("M {path}"));
                        }
                    }
                }
            }
        }
        // Remember what each file was, so /undo can restore it.
        let mut batch = rig_core::tool::UndoBatch::default();
        for (path, _) in &writes {
            if !batch.files.iter().any(|(p, _)| p == path) {
                batch.files.push((path.clone(), fs::read(path).ok()));
            }
        }
        for (path, content) in writes {
            let res = match content {
                Some(text) => path
                    .parent()
                    .map_or(Ok(()), fs::create_dir_all)
                    .and_then(|_| fs::write(&path, text)),
                None => fs::remove_file(&path),
            };
            if let Err(e) = res {
                return ToolOutput::err(format!("failed writing {}: {e}", path.display()));
            }
        }
        ctx.shared.undo.locked().push(batch);
        ToolOutput::ok(format!("Patch applied:\n{}", report.join("\n")))
    }
}

#[cfg(test)]
mod tests;
