//! `ask_user`: a question for the person at the keyboard, with options to pick from. The user can
//! always type an answer of their own, so the options are suggestions, not a limit.

use rig_core::provider::ToolSpec;
use rig_core::tool::{Access, Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};

/// Most options shown. More than this is hard to scan in a terminal.
pub const MAX_OPTIONS: usize = 6;

pub struct AskUser;

/// The question and its options, as the model sent them. Blank options are dropped.
pub fn parse(args: &Value) -> Option<(String, Vec<String>)> {
    let question = args.get("question")?.as_str()?.trim().to_string();
    if question.is_empty() {
        return None;
    }
    let options = args
        .get("options")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|o| !o.is_empty())
                .take(MAX_OPTIONS)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    Some((question, options))
}

impl Tool for AskUser {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "ask_user".into(),
            description: format!(
                "Ask the person a question and wait for the answer. Use it only when a choice would change the result and you cannot find the answer yourself. Give up to {MAX_OPTIONS} short options when there are clear choices; the person can always type their own answer instead. Ask one question at a time."
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "question": {"type": "string", "description": "The question, complete enough to answer without more context."},
                    "options": {
                        "type": "array",
                        "items": {"type": "string"},
                        "maxItems": MAX_OPTIONS,
                        "description": "Short choices, most likely first. Leave out for a free-form question."
                    }
                },
                "required": ["question"]
            }),
        }
    }

    fn access(&self, args: &Value) -> Access {
        match parse(args) {
            Some((question, options)) => Access::Ask { question, options },
            None => Access::Ask {
                question: String::new(),
                options: Vec::new(),
            },
        }
    }

    fn describe(&self, args: &Value) -> String {
        parse(args).map_or_else(|| "question".into(), |(q, _)| format!("ask: {q}"))
    }

    fn run(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        // Answered by the front end before this runs (see the agent's planning step).
        match parse(args) {
            Some(_) => ToolOutput::err("ask_user needs an interactive session to answer"),
            None => ToolOutput::err("ask_user needs a non-empty 'question'"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_trimmed_capped_and_blanks_dropped() {
        let args = json!({"question": " Which DB? ", "options": ["Postgres", " ", "SQLite", "a", "b", "c", "d", "e"]});
        let (q, o) = parse(&args).unwrap();
        assert_eq!(q, "Which DB?");
        assert_eq!(o, ["Postgres", "SQLite", "a", "b", "c", "d"]);
    }

    #[test]
    fn a_question_is_required_and_options_are_optional() {
        assert!(parse(&json!({"question": "  "})).is_none());
        assert!(parse(&json!({})).is_none());
        assert_eq!(
            parse(&json!({"question": "Name?"})).unwrap().1,
            Vec::<String>::new()
        );
    }
}
