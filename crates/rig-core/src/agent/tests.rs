use super::*;
use crate::message::{Block, Role};
use crate::permission::Autonomy;
use crate::provider::{Completion, StopReason, Usage};
use crate::tool::{Access, Tool, ToolOutput};
use serde_json::{Value, json};
use std::sync::Mutex;

/// Plays back a script of assistant messages, recording what it was sent.
struct Script {
    steps: Mutex<Vec<Result<Message, ProviderError>>>,
    seen: Mutex<Vec<usize>>,
}

impl Provider for Script {
    fn name(&self) -> &str {
        "script"
    }
    fn model(&self) -> &str {
        "script"
    }
    fn stream(
        &self,
        req: &Request,
        on: &mut dyn FnMut(StreamEvent),
    ) -> Result<Completion, ProviderError> {
        self.seen.locked().push(req.messages.len());
        let step = self.steps.locked().remove(0)?;
        on(StreamEvent::Text(&step.text()));
        let stop = if step.tool_calls().next().is_some() {
            StopReason::ToolUse
        } else {
            StopReason::EndTurn
        };
        Ok(Completion {
            message: step,
            usage: Usage {
                input: 100,
                output: 10,
                cached: 0,
            },
            stop,
        })
    }
}

struct Echo;
impl Tool for Echo {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "Echo".into(),
            description: "".into(),
            parameters: json!({"type": "object"}),
        }
    }
    fn access(&self, _: &Value) -> Access {
        Access::Read
    }
    fn describe(&self, _: &Value) -> String {
        "echo".into()
    }
    fn run(&self, args: &Value, _: &ToolCtx) -> ToolOutput {
        ToolOutput::ok(format!("echo:{}", args["x"]))
    }
}

fn asst(blocks: Vec<Block>) -> Message {
    Message {
        role: Role::Assistant,
        blocks,
    }
}

fn agent(steps: Vec<Result<Message, ProviderError>>) -> Agent {
    let p = Script {
        steps: Mutex::new(steps),
        seen: Mutex::new(vec![]),
    };
    Agent::new(
        Box::new(p),
        Registry::new(vec![Box::new(Echo)]),
        Policy::new(Autonomy::Low),
        "sys".into(),
        ToolCtx::new(std::env::temp_dir(), "t".into()),
        vec![Message::user("<rig-context>env</rig-context>")],
    )
}

#[derive(Default)]
struct Rec {
    saw_tool: bool,
}
impl Ui for Rec {
    fn tool_start(&mut self, _: &str, _: &str) {
        self.saw_tool = true;
    }
}

#[test]
fn tool_loop_runs_and_finishes() {
    let call = Block::ToolCall {
        id: "c1".into(),
        name: "Echo".into(),
        args: json!({"x": 1}),
    };
    let mut a = agent(vec![
        Ok(asst(vec![call])),
        Ok(asst(vec![Block::Text {
            text: "done".into(),
        }])),
    ]);
    let mut ui = Rec::default();
    a.run_text("go", &mut ui).unwrap();
    assert!(ui.saw_tool);
    let last = a.history.last().unwrap();
    assert_eq!(last.text(), "done");
    let results: Vec<_> = a
        .history
        .iter()
        .flat_map(|m| &m.blocks)
        .filter_map(|b| {
            if let Block::ToolResult { output, .. } = b {
                Some(output.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(results, ["echo:1"]);
}

#[test]
fn a_prompt_nothing_answered_is_rolled_back_so_a_retry_is_not_sent_twice() {
    let mut a = agent(vec![
        Err(ProviderError::Auth("expired".into())),
        Ok(asst(vec![Block::Text { text: "hi".into() }])),
    ]);
    let before = a.history.len();
    #[derive(Default)]
    struct Rolled(Option<usize>);
    impl Ui for Rolled {
        fn rolled_back(&mut self, len: usize) {
            self.0 = Some(len);
        }
    }
    let mut ui = Rolled::default();
    assert!(a.run_text("hello", &mut ui).is_err());
    assert_eq!((a.history.len(), ui.0), (before, Some(before)));
    a.run_text("hello", &mut ui).unwrap();
    assert_eq!(a.history.iter().filter(|m| m.text() == "hello").count(), 1);
}

#[test]
fn a_failure_after_the_model_already_answered_keeps_the_history() {
    let call = Block::ToolCall {
        id: "c1".into(),
        name: "Echo".into(),
        args: json!({}),
    };
    let mut a = agent(vec![
        Ok(asst(vec![call])),
        Err(ProviderError::Auth("expired".into())),
    ]);
    let before = a.history.len();
    assert!(a.run_text("go", &mut Rec::default()).is_err());
    assert!(
        a.history.len() > before + 1,
        "the tool exchange stays so the work is not lost"
    );
}

#[test]
fn steering_is_delivered_before_the_next_model_step() {
    let call = Block::ToolCall {
        id: "c1".into(),
        name: "Echo".into(),
        args: json!({}),
    };
    let mut a = agent(vec![
        Ok(asst(vec![call])),
        Ok(asst(vec![Block::Text { text: "ok".into() }])),
    ]);
    struct Steer(bool);
    impl Ui for Steer {
        fn steering(&mut self) -> Vec<Message> {
            // Only after the first model step has run (tools executed).
            if std::mem::replace(&mut self.0, true) {
                vec![Message::user("actually, use the other file")]
            } else {
                vec![]
            }
        }
    }
    a.run_text("go", &mut Steer(false)).unwrap();
    let texts: Vec<String> = a.history.iter().map(Message::text).collect();
    assert!(texts.iter().any(|t| t == "actually, use the other file"));
    let steer_at = texts
        .iter()
        .position(|t| t == "actually, use the other file")
        .unwrap();
    let result_at = a
        .history
        .iter()
        .position(|m| {
            m.blocks
                .iter()
                .any(|b| matches!(b, Block::ToolResult { .. }))
        })
        .unwrap();
    assert!(
        steer_at > result_at,
        "steering must follow the tool results it interrupts"
    );
}

#[test]
fn cancel_stops_the_turn_without_leaving_dangling_calls() {
    let call = Block::ToolCall {
        id: "c1".into(),
        name: "Echo".into(),
        args: json!({}),
    };
    let mut a = agent(vec![Ok(asst(vec![call]))]);
    struct CancelOnTool(std::sync::Arc<crate::tool::Shared>);
    impl Ui for CancelOnTool {
        fn text(&mut self, _: &str) {
            self.0.cancel.store(true, Ordering::Relaxed);
        }
    }
    let shared = a.ctx.shared.clone();
    let r = a.run_text("go", &mut CancelOnTool(shared));
    assert!(matches!(r, Err(AgentError::Cancelled)) || r.is_ok());
    // Every tool call in history has a matching result.
    let calls = a
        .history
        .iter()
        .flat_map(|m| &m.blocks)
        .filter(|b| matches!(b, Block::ToolCall { .. }))
        .count();
    let results = a
        .history
        .iter()
        .flat_map(|m| &m.blocks)
        .filter(|b| matches!(b, Block::ToolResult { .. }))
        .count();
    assert_eq!(calls, results);
}

#[test]
fn auto_compaction_needs_new_growth_after_a_compaction() {
    let mut a = agent(vec![]);
    a.context_window = 1_000;
    a.auto_compact_percent = 10;
    a.history.push(Message::user("x".repeat(2_000)));
    assert!(a.should_compact());
    a.compact_floor = a.context_tokens() + 100;
    assert!(!a.should_compact(), "no growth since the last compaction");
    a.history.push(Message::user("y".repeat(1_000)));
    assert!(a.should_compact(), "enough new material");
}

#[test]
fn overflow_triggers_one_compaction_and_retry() {
    let overflow = ProviderError::Http {
        status: 400,
        body: "{\"error\":{\"code\":\"context_length_exceeded\"}}".into(),
    };
    let mut a = agent(vec![
        Err(overflow),
        Ok(asst(vec![Block::Text {
            text: "recovered".into(),
        }])),
    ]);
    // Nothing compactable and no archive: the error must surface rather than loop.
    let r = a.run_text("go", &mut Rec::default());
    assert!(matches!(r, Err(AgentError::Provider(_))));
}
