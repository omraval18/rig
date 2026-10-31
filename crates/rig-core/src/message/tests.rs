use super::*;
use serde_json::json;

fn call(id: &str) -> Block {
    Block::ToolCall {
        id: id.into(),
        name: "run_command".into(),
        args: json!({}),
    }
}

#[test]
fn an_unanswered_call_gets_an_error_result_and_answered_ones_are_left_alone() {
    let mut h = vec![
        Message::user("go"),
        Message {
            role: Role::Assistant,
            blocks: vec![call("a"), call("b")],
        },
        Message::tool_results(vec![Block::ToolResult {
            id: "a".into(),
            output: "ok".into(),
            is_error: false,
        }]),
        Message {
            role: Role::Assistant,
            blocks: vec![call("c")],
        },
    ];
    assert_eq!(repair_dangling_calls(&mut h), 2);
    // "b" joins the existing results message; "c" at the end gets a new one.
    assert_eq!(h[2].blocks.len(), 2);
    assert_eq!(h.len(), 5);
    assert!(matches!(&h[4].blocks[0], Block::ToolResult { id, is_error: true, .. } if id == "c"));
    assert_eq!(repair_dangling_calls(&mut h), 0, "idempotent");
}
