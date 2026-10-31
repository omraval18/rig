use super::*;

#[test]
fn pictures_go_out_as_input_image_data_urls() {
    let m = Message {
        role: Role::User,
        blocks: vec![
            Block::Text {
                text: "what is this".into(),
            },
            Block::Image {
                mime: "image/png".into(),
                data: "QUJD".into(),
            },
        ],
    };
    let input = to_input(&[m]);
    assert_eq!(
        input[0]["content"][1],
        json!({"type": "input_image", "image_url": "data:image/png;base64,QUJD"})
    );
}

#[test]
fn history_converts_to_responses_items() {
    let msgs = vec![
        Message::user("hi"),
        Message {
            role: Role::Assistant,
            blocks: vec![
                Block::Reasoning {
                    text: "t".into(),
                    raw: Some(Opaque {
                        provider: WIRE.into(),
                        data: json!({"type": "reasoning", "id": "rs_1", "encrypted_content": "x"}),
                    }),
                },
                Block::Reasoning {
                    text: "other".into(),
                    raw: Some(Opaque {
                        provider: "someone-else".into(),
                        data: json!({}),
                    }),
                },
                Block::Text { text: "ok".into() },
                Block::ToolCall {
                    id: "c1".into(),
                    name: "read_file".into(),
                    args: json!({"file_path": "/x"}),
                },
            ],
        },
        Message::tool_results(vec![Block::ToolResult {
            id: "c1".into(),
            output: "data".into(),
            is_error: false,
        }]),
    ];
    let items = to_input(&msgs);
    let kinds: Vec<_> = items
        .iter()
        .map(|i| {
            i["type"]
                .as_str()
                .or(i["role"].as_str())
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "user",
            "reasoning",
            "assistant",
            "function_call",
            "function_call_output"
        ]
    );
    assert_eq!(items[3]["arguments"], "{\"file_path\":\"/x\"}");
}

#[test]
fn output_items_become_blocks() {
    assert!(matches!(
        item_to_block(
            &json!({"type": "message", "content": [{"type": "output_text", "text": "hey"}]})
        ),
        Some(Block::Text { .. })
    ));
    assert!(matches!(
        item_to_block(
            &json!({"type": "function_call", "call_id": "c", "name": "list_dir", "arguments": "{}"})
        ),
        Some(Block::ToolCall { .. })
    ));
    assert!(item_to_block(&json!({"type": "web_search_call"})).is_none());
}
