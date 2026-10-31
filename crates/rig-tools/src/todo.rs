use rig_core::provider::ToolSpec;
use rig_core::sync::MutexExt;
use rig_core::tool::{Access, Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};
use std::sync::Mutex;

/// Session-scoped task list the model keeps for itself. Replaces the whole list on every call.
#[derive(Default)]
pub struct UpdateTasks {
    items: Mutex<Vec<(String, String)>>,
}

const STATUSES: [&str; 3] = ["open", "active", "done"];

impl Tool for UpdateTasks {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "update_tasks".into(),
            description: "Maintain a short task list for multi-step work. Send the complete list each time. Keep at most one task active and mark tasks done as soon as they are done. Skip it for trivial requests.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "text": {"type": "string"},
                                "state": {"type": "string", "enum": STATUSES}
                            },
                            "required": ["text", "state"]
                        }
                    }
                },
                "required": ["tasks"]
            }),
        }
    }

    fn access(&self, _: &Value) -> Access {
        Access::Read
    }

    fn describe(&self, args: &Value) -> String {
        let n = args
            .get("tasks")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        format!("{n} items")
    }

    fn summarize(&self, _: &Value, out: &ToolOutput) -> Option<String> {
        if out.is_error {
            return None;
        }
        let list: Vec<String> = out
            .text
            .lines()
            .skip(1)
            .map(|l| match l.split_at_checked(4) {
                Some(("[x] ", rest)) => format!("☑ {rest}"),
                Some(("[~] ", rest)) => format!("◐ {rest}"),
                Some(("[ ] ", rest)) => format!("☐ {rest}"),
                _ => l.to_string(),
            })
            .collect();
        (!list.is_empty()).then(|| list.join("\n"))
    }

    fn run(&self, args: &Value, _: &ToolCtx) -> ToolOutput {
        let Some(list) = args.get("tasks").and_then(Value::as_array) else {
            return ToolOutput::err("tasks must be an array");
        };
        let mut next = Vec::with_capacity(list.len());
        for t in list {
            let content = t.get("text").and_then(Value::as_str).unwrap_or("").trim();
            let status = t.get("state").and_then(Value::as_str).unwrap_or("open");
            if content.is_empty() || !STATUSES.contains(&status) {
                return ToolOutput::err(
                    "each task needs non-empty text and a state of open, active or done",
                );
            }
            next.push((content.to_string(), status.to_string()));
        }
        let rendered: Vec<String> = next
            .iter()
            .map(|(c, s)| {
                let mark = match s.as_str() {
                    "done" => "[x]",
                    "active" => "[~]",
                    _ => "[ ]",
                };
                format!("{mark} {c}")
            })
            .collect();
        *self.items.locked() = next;
        ToolOutput::ok(format!("Todo list updated:\n{}", rendered.join("\n")))
    }
}

#[cfg(test)]
mod summary_tests {
    use super::*;

    #[test]
    fn plan_renders_as_a_checklist() {
        let t = UpdateTasks::default();
        let out = t.run(&json!({"tasks": [{"text": "write test", "state": "done"}, {"text": "fix bug", "state": "active"}, {"text": "ship", "state": "open"}]}), &ToolCtx::new(".".into(), "s".into()));
        assert_eq!(
            t.summarize(&Value::Null, &out).as_deref(),
            Some("☑ write test\n◐ fix bug\n☐ ship")
        );
    }
}
