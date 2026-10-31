//! Provider-agnostic conversation model.
//!
//! Every provider adapter converts to and from these types, so the agent loop,
//! sessions and tools never see a wire format.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// Provider-owned replay state, e.g. an encrypted Responses reasoning item.
/// Only the provider that produced it may send it back.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Opaque {
    pub provider: String,
    pub data: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw: Option<Opaque>,
    },
    ToolCall {
        id: String,
        name: String,
        args: Value,
    },
    ToolResult {
        id: String,
        output: String,
        #[serde(default)]
        is_error: bool,
    },
    /// A picture the user attached. `data` is base64.
    Image {
        mime: String,
        data: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub blocks: Vec<Block>,
}

/// Give every tool call that never got a result an error result, so the history is valid to send.
/// A turn killed mid-tool (Esc at the wrong moment, a crash, a closed terminal) leaves such calls
/// behind, and providers reject a history that has them. Returns how many calls were repaired.
pub fn repair_dangling_calls(history: &mut Vec<Message>) -> usize {
    let mut repaired = 0;
    let mut i = 0;
    while i < history.len() {
        if history[i].role == Role::Assistant {
            let ids: Vec<String> = history[i]
                .tool_calls()
                .map(|(id, _, _)| id.to_string())
                .collect();
            if !ids.is_empty() {
                let answered: Vec<String> = history
                    .get(i + 1)
                    .map(|m| {
                        m.blocks
                            .iter()
                            .filter_map(|b| {
                                if let Block::ToolResult { id, .. } = b {
                                    Some(id.clone())
                                } else {
                                    None
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let missing: Vec<Block> = ids
                    .into_iter()
                    .filter(|id| !answered.contains(id))
                    .map(|id| Block::ToolResult { id, output: "The tool call was interrupted before it finished; no result is available.".into(), is_error: true })
                    .collect();
                if !missing.is_empty() {
                    repaired += missing.len();
                    let next_is_results = history.get(i + 1).is_some_and(|m| {
                        m.role == Role::User
                            && m.blocks
                                .iter()
                                .any(|b| matches!(b, Block::ToolResult { .. }))
                    });
                    if next_is_results {
                        history[i + 1].blocks.extend(missing);
                    } else {
                        history.insert(i + 1, Message::tool_results(missing));
                    }
                }
            }
        }
        i += 1;
    }
    repaired
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            blocks: vec![Block::Text { text: text.into() }],
        }
    }

    pub fn tool_results(results: Vec<Block>) -> Self {
        Self {
            role: Role::User,
            blocks: results,
        }
    }

    /// Concatenated visible text.
    pub fn text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn tool_calls(&self) -> impl Iterator<Item = (&str, &str, &Value)> {
        self.blocks.iter().filter_map(|b| match b {
            Block::ToolCall { id, name, args } => Some((id.as_str(), name.as_str(), args)),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests;
