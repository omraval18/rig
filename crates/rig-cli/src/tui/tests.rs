use super::format::{explain, fmt_duration, fmt_tokens};
use super::*;
use rig_core::agent::AgentError;
use rig_core::message::Block;
use std::time::Duration;

#[test]
fn errors_are_explained_and_retryable_ones_say_so() {
    use rig_core::provider::ProviderError as P;
    let (m, retry) = explain(
        &AgentError::Provider(P::Network("io: Connection refused (os error 61)".into())),
        "chatgpt",
    );
    assert!(m.starts_with("Can't reach chatgpt") && retry, "{m}");
    let (m, retry) = explain(
        &AgentError::Provider(P::Auth(
            "Not signed in. Type /login to use your ChatGPT plan.".into(),
        )),
        "chatgpt",
    );
    assert!(m.starts_with("Not signed in") && !retry);
    assert!(
        explain(
            &AgentError::Provider(P::Http {
                status: 503,
                body: String::new()
            }),
            "x"
        )
        .1
    );
    assert!(!explain(&AgentError::TurnLimit(3), "x").1);
}

#[test]
fn durations() {
    assert_eq!(fmt_duration(Duration::from_secs(45)), "45s");
    assert_eq!(fmt_duration(Duration::from_secs(125)), "2m 05s");
}

#[test]
fn token_formatting() {
    assert_eq!(fmt_tokens(999), "999");
    assert_eq!(fmt_tokens(1500), "1.5k");
    assert_eq!(fmt_tokens(272_000), "272k");
    assert_eq!(fmt_tokens(2_500_000), "2.5M");
}

#[test]
fn visible_text_drops_injected_context() {
    let m = Message {
        role: Role::User,
        blocks: vec![
            Block::Text {
                text: "fix @a.rs".into(),
            },
            Block::Text {
                text: "<rig-context>\nfile body\n</rig-context>".into(),
            },
        ],
    };
    assert_eq!(visible_text(&m), "fix @a.rs");
}

#[test]
fn reminders_are_hidden_from_recaps_but_prompts_are_not() {
    assert!(is_hidden(&Message::user(
        "<rig-context>\nenv\n</rig-context>"
    )));
    assert!(!is_hidden(&Message::user("fix the bug")));
}
