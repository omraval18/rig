use super::*;
use crate::provider::{Completion, StopReason, StreamEvent, Usage};
use serde_json::json;

struct Notes(&'static str);
impl Provider for Notes {
    fn name(&self) -> &str {
        "fake"
    }
    fn model(&self) -> &str {
        "fake"
    }
    fn stream(
        &self,
        _: &Request,
        _: &mut dyn FnMut(StreamEvent),
    ) -> Result<Completion, ProviderError> {
        Ok(Completion {
            message: Message {
                role: Role::Assistant,
                blocks: vec![Block::Text {
                    text: self.0.into(),
                }],
            },
            usage: Usage::default(),
            stop: StopReason::EndTurn,
        })
    }
}

fn assistant(blocks: Vec<Block>) -> Message {
    Message {
        role: Role::Assistant,
        blocks,
    }
}

fn call(id: &str, name: &str) -> Block {
    Block::ToolCall {
        id: id.into(),
        name: name.into(),
        args: json!({"file_path": "/src/lib.rs"}),
    }
}

fn result(id: &str, out: &str) -> Message {
    Message::tool_results(vec![Block::ToolResult {
        id: id.into(),
        output: out.into(),
        is_error: false,
    }])
}

/// reminder, then `turns` turns of: user, assistant(call), result, assistant(final).
fn convo(turns: usize) -> Vec<Message> {
    let mut h = vec![Message::user("<rig-context>env</rig-context>")];
    for i in 0..turns {
        h.push(Message::user(format!(
            "task number {i}: please fix the parser"
        )));
        h.push(assistant(vec![call(&format!("c{i}"), "read_file")]));
        h.push(result(&format!("c{i}"), &"file body line\n".repeat(400)));
        h.push(assistant(vec![Block::Text {
            text: format!("done with {i}"),
        }]));
    }
    h
}

#[test]
fn split_keeps_last_two_turns_and_never_cuts_a_call_from_its_result() {
    let h = convo(5);
    let s = split_point(&h, 50_000).unwrap();
    assert!(is_turn_start(&h[s]));
    assert_eq!(h.len() - s, 8, "last two turns of four messages");
    for (i, m) in h.iter().enumerate().skip(1) {
        if m.blocks
            .iter()
            .any(|b| matches!(b, Block::ToolResult { .. }))
        {
            assert!(!(i == s), "split must not land on a tool result");
        }
    }
}

#[test]
fn one_long_turn_splits_at_an_assistant_message() {
    let mut h = vec![
        Message::user("<rig-context>env</rig-context>"),
        Message::user("do a big refactor"),
    ];
    for i in 0..12 {
        h.push(assistant(vec![call(&format!("c{i}"), "read_file")]));
        h.push(result(&format!("c{i}"), &"x".repeat(4000)));
    }
    let s = split_point(&h, 3_000).unwrap();
    assert_eq!(h[s].role, Role::Assistant);
    assert!(s > 2 && s < h.len() - 2);
}

#[test]
fn compaction_replaces_head_archives_it_and_flags_invented_handles() {
    let dir = std::env::temp_dir().join(format!("rig-compact-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("a.jsonl");
    let _ = std::fs::remove_file(&archive);
    let mut h = convo(5);
    let before = h.len();
    let cancel = AtomicBool::new(false);
    let cp = compact(
        &Notes("## Goal\nFix parser [T1] and [T99] per [M1]."),
        &mut h,
        &archive,
        50_000,
        "s",
        &cancel,
    )
    .unwrap()
    .unwrap();

    assert_eq!(h.len(), before - (cp.split - 1) + 1);
    assert!(h[0].text().contains("env"));
    let c = h[1].text();
    assert!(is_checkpoint(&h[1]));
    assert!(
        c.contains("[M1] task number 0") && c.contains("done with 0"),
        "exact user messages and replies kept"
    );
    assert!(c.contains("[T1]") && c.contains("(unverified)") && !c.contains("T99"));
    assert_eq!(cp.unverified_handles, 1);

    assert!(lookup(&archive, "T1").unwrap().contains("file body line"));
    let hits = search(&archive, &["parser".into(), "task number 1".into()], 5);
    assert_eq!(hits.len(), 1);
    assert!(search(&archive, &["nonexistent phrase".into()], 5).is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn small_conversations_are_left_alone() {
    let mut h = convo(1);
    let cancel = AtomicBool::new(false);
    let dir = std::env::temp_dir().join("rig-compact-none");
    assert!(
        compact(
            &Notes("x"),
            &mut h,
            &dir.join("a.jsonl"),
            50_000,
            "s",
            &cancel
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn second_compaction_continues_ids() {
    let dir = std::env::temp_dir().join(format!("rig-compact2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("a.jsonl");
    let _ = std::fs::remove_file(&archive);
    let cancel = AtomicBool::new(false);
    let mut h = convo(5);
    compact(&Notes("n1"), &mut h, &archive, 50_000, "s", &cancel)
        .unwrap()
        .unwrap();
    for i in 5..9 {
        h.push(Message::user(format!(
            "task number {i}: please fix the parser"
        )));
        h.push(assistant(vec![call(&format!("c{i}"), "read_file")]));
        h.push(result(&format!("c{i}"), &"file body line\n".repeat(400)));
        h.push(assistant(vec![Block::Text {
            text: format!("done with {i}"),
        }]));
    }
    compact(&Notes("n2"), &mut h, &archive, 50_000, "s", &cancel)
        .unwrap()
        .unwrap();
    let ids = known_ids(&archive);
    assert!(
        ids.contains("T1") && ids.contains("T4") && ids.contains("T5"),
        "{ids:?}"
    );
    std::fs::remove_dir_all(dir).ok();
}
