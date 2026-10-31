//! `update_goal`: the model says whether the goal it was given is finished or blocked. The front
//! end keeps the loop going until it hears one of these, or runs out of rounds.

use rig_core::provider::ToolSpec;
use rig_core::sync::MutexExt;
use rig_core::tool::{Access, Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};

pub struct UpdateGoal;

const STATUSES: [&str; 2] = ["complete", "blocked"];

impl Tool for UpdateGoal {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "update_goal".into(),
            description: "Report the state of the goal you are working toward in a /loop. Call it with 'complete' only when the whole goal is done and verified. Call it with 'blocked' only when you cannot continue without the person and have already asked them with ask_user. Otherwise keep working and do not call it.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "status": {"type": "string", "enum": STATUSES},
                    "note": {"type": "string", "description": "One or two sentences: what was done, or what blocks it."}
                },
                "required": ["status"]
            }),
        }
    }

    fn access(&self, _: &Value) -> Access {
        Access::Read
    }

    fn describe(&self, args: &Value) -> String {
        args.get("status")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string()
    }

    fn run(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(status) = args.get("status").and_then(Value::as_str) else {
            return ToolOutput::err("update_goal needs a status");
        };
        if !STATUSES.contains(&status) {
            return ToolOutput::err("status must be 'complete' or 'blocked'");
        }
        *ctx.shared.goal_signal.locked() = Some(status.to_string());
        ToolOutput::ok(format!("Goal recorded as {status}."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_status_is_recorded_for_the_front_end_and_bad_ones_are_refused() {
        let ctx = ToolCtx::new(PathBuf::from("."), "t".into());
        assert!(UpdateGoal.run(&json!({"status": "maybe"}), &ctx).is_error);
        assert!(
            !UpdateGoal
                .run(&json!({"status": "complete"}), &ctx)
                .is_error
        );
        assert_eq!(ctx.shared.goal_signal.locked().as_deref(), Some("complete"));
    }
}
