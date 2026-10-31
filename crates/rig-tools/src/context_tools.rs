//! Tools that manage the model's own context: activating skills and reading back compacted history.

use crate::util::{str_arg, str_list};
use rig_core::compact;
use rig_core::provider::ToolSpec;
use rig_core::skills;
use rig_core::sync::MutexExt;
use rig_core::sync::RwLockExt;
use rig_core::tool::{Access, Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};

pub struct UseSkill;

impl Tool for UseSkill {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "use_skill".into(),
            description: "Activate one skill by its exact name from the skills list in the session context. Use a relevant skill proactively. Never invent a name, and do not re-activate a skill that is already active.".into(),
            parameters: json!({
                "type": "object",
                "properties": {"name": {"type": "string", "description": "Exact skill name."}},
                "required": ["name"]
            }),
        }
    }

    fn access(&self, _: &Value) -> Access {
        Access::Read
    }

    fn describe(&self, args: &Value) -> String {
        str_arg(args, "name").unwrap_or("?").to_string()
    }

    fn run(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(name) = str_arg(args, "name") else {
            return ToolOutput::err("skill is required");
        };
        let list = ctx.shared.skills.read_locked();
        match list.iter().find(|s| s.name == name.trim_start_matches('$')) {
            Some(s) => match skills::activation_text(s) {
                Ok(t) => ToolOutput::ok(t),
                Err(e) => ToolOutput::err(format!("cannot read skill '{name}': {e}")),
            },
            None => {
                let names: Vec<&str> = list.iter().map(|s| s.name.as_str()).collect();
                ToolOutput::err(format!(
                    "no skill named '{name}'. Available: {}",
                    if names.is_empty() {
                        "none".into()
                    } else {
                        names.join(", ")
                    }
                ))
            }
        }
    }
}

pub struct Recall;

impl Tool for Recall {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "recall".into(),
            description: "Look up earlier conversation that was compacted out of your context. Pass `words` (all must appear) to search messages and tool calls, or `handle` (like T7 or M12, as cited in the compaction notes) to read one in full. Use it before guessing at details you no longer see.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "words": {"type": "array", "items": {"type": "string"}, "description": "1-5 words or phrases; every one must appear."},
                    "handle": {"type": "string", "description": "A handle such as T7 (tool call) or M12 (message)."}
                }
            }),
        }
    }

    fn access(&self, _: &Value) -> Access {
        Access::Read
    }

    fn describe(&self, args: &Value) -> String {
        str_arg(args, "handle")
            .map(str::to_string)
            .unwrap_or_else(|| str_list(args, "words").join(" + "))
    }

    fn run(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let archive = ctx.shared.archive.locked().clone();
        let Some(archive) = archive.filter(|p| p.is_file()) else {
            return ToolOutput::ok(
                "Nothing has been compacted in this session yet, so there is no archived history.",
            );
        };
        if let Some(id) = str_arg(args, "handle") {
            return match compact::lookup(&archive, id.trim()) {
                Some(t) => ToolOutput::ok(t),
                None => ToolOutput::err(format!("no archived record '{id}'")),
            };
        }
        let phrases = str_list(args, "words");
        if phrases.is_empty() {
            return ToolOutput::err("give `phrases` or an `id`");
        }
        let hits = compact::search(&archive, &phrases[..phrases.len().min(5)], 15);
        if hits.is_empty() {
            ToolOutput::ok("No archived message or tool call contains all of those phrases.")
        } else {
            ToolOutput::ok(format!(
                "{} match(es):\n\n{}",
                hits.len(),
                hits.join("\n\n")
            ))
        }
    }
}
