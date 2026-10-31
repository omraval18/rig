use super::*;

#[test]
fn pictures_go_out_as_base64_image_sources() {
    let m = Message {
        role: Role::User,
        blocks: vec![
            Block::Image {
                mime: "image/png".into(),
                data: "QUJD".into(),
            },
            Block::Text {
                text: "this?".into(),
            },
        ],
    };
    assert_eq!(
        to_messages(&[m])[0]["content"][0],
        json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "QUJD"}})
    );
}

#[test]
fn converts_history_and_drops_foreign_reasoning() {
    let msgs = vec![
        Message::user("go"),
        Message {
            role: Role::Assistant,
            blocks: vec![
                Block::Reasoning {
                    text: "x".into(),
                    raw: None,
                },
                Block::ToolCall {
                    id: "t1".into(),
                    name: "list_dir".into(),
                    args: json!({}),
                },
            ],
        },
        Message::tool_results(vec![Block::ToolResult {
            id: "t1".into(),
            output: "a".into(),
            is_error: true,
        }]),
    ];
    let v = to_messages(&msgs);
    assert_eq!(v.len(), 3);
    assert_eq!(v[1]["content"][0]["type"], "tool_use");
    assert_eq!(v[2]["content"][0]["tool_use_id"], "t1");
    assert_eq!(v[2]["content"][0]["is_error"], true);
}
