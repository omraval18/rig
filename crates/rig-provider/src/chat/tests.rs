use super::*;

#[test]
fn pictures_make_the_user_content_a_list_of_parts() {
    let m = Message {
        role: Role::User,
        blocks: vec![
            Block::Text {
                text: "this?".into(),
            },
            Block::Image {
                mime: "image/png".into(),
                data: "QUJD".into(),
            },
        ],
    };
    let out = to_messages("sys", &[m]);
    assert_eq!(
        out[1]["content"][0],
        json!({"type": "text", "text": "this?"})
    );
    assert_eq!(
        out[1]["content"][1]["image_url"]["url"],
        "data:image/png;base64,QUJD"
    );
}

#[test]
fn tool_round_trip_shape() {
    let msgs = vec![
        Message::user("q"),
        Message {
            role: Role::Assistant,
            blocks: vec![Block::ToolCall {
                id: "c1".into(),
                name: "list_dir".into(),
                args: json!({"a": 1}),
            }],
        },
        Message::tool_results(vec![Block::ToolResult {
            id: "c1".into(),
            output: "out".into(),
            is_error: false,
        }]),
    ];
    let v = to_messages("sys", &msgs);
    let roles: Vec<_> = v.iter().map(|m| m["role"].as_str().unwrap()).collect();
    assert_eq!(roles, ["system", "user", "assistant", "tool"]);
    assert_eq!(v[2]["content"], Value::Null);
    assert_eq!(v[2]["tool_calls"][0]["function"]["arguments"], "{\"a\":1}");
    assert_eq!(v[3]["tool_call_id"], "c1");
}
