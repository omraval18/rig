//! Anthropic Messages API over SSE (API-key auth).

use crate::http::{Endpoint, auth_rejected, get_json, post_json, quick_agent, stream_events};
use rig_core::message::{Block, Message, Role};
use rig_core::provider::*;
use serde_json::{Value, json};

const API_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u64 = 16_384;

pub struct Anthropic {
    ep: Endpoint,
    max_tokens: u64,
}

impl Anthropic {
    pub(crate) fn new(ep: Endpoint) -> Self {
        Self {
            ep,
            max_tokens: DEFAULT_MAX_TOKENS,
        }
    }

    fn body(&self, req: &Request) -> Value {
        let mut messages = to_messages(req.messages);
        // Cache everything up to the newest user turn so each step only pays for what is new.
        let newest_block = messages
            .iter_mut()
            .rev()
            .find(|m| m["role"] == "user")
            .and_then(|m| m["content"].as_array_mut())
            .and_then(|c| c.last_mut());
        if let Some(b) = newest_block {
            b["cache_control"] = json!({"type": "ephemeral"});
        }
        json!({
            "model": self.ep.model,
            "max_tokens": self.max_tokens,
            "stream": true,
            "system": [{"type": "text", "text": req.system, "cache_control": {"type": "ephemeral"}}],
            "messages": messages,
            "tools": req.tools.iter().map(|t| json!({
                "name": t.name, "description": t.description, "input_schema": t.parameters,
            })).collect::<Vec<_>>(),
        })
    }
}

fn to_messages(messages: &[Message]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|m| {
            let content: Vec<Value> = m
                .blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } if !text.is_empty() => Some(json!({"type": "text", "text": text})),
                    Block::ToolCall { id, name, args } => Some(json!({"type": "tool_use", "id": id, "name": name, "input": args})),
                    Block::ToolResult { id, output, is_error } => {
                        Some(json!({"type": "tool_result", "tool_use_id": id, "content": output, "is_error": is_error}))
                    }
                    Block::Image { mime, data } => Some(json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": data}})),
                    // Reasoning from other providers cannot be replayed here.
                    _ => None,
                })
                .collect();
            (!content.is_empty()).then(|| json!({"role": if m.role == Role::User { "user" } else { "assistant" }, "content": content}))
        })
        .collect()
}

enum Partial {
    Text(String),
    Tool {
        id: String,
        name: String,
        json: String,
    },
    Skip,
}

impl Provider for Anthropic {
    fn name(&self) -> &str {
        &self.ep.name
    }

    fn model(&self) -> &str {
        &self.ep.model
    }

    fn stream(
        &self,
        req: &Request,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<Completion, ProviderError> {
        let mut headers = self.ep.auth.headers(false)?;
        headers.push(("anthropic-version".into(), API_VERSION.into()));
        headers.push(("Accept".into(), "text/event-stream".into()));
        let resp = post_json(
            &self.ep.http,
            &self.ep.url("/v1/messages"),
            &headers,
            &self.body(req),
        )
        .map_err(auth_rejected)?;

        let mut parts: Vec<Partial> = Vec::new();
        let mut usage = Usage::default();
        let mut stop_reason = String::new();
        let mut finished = false;
        stream_events(resp, req.cancel, |_, data| {
            let Ok(ev) = serde_json::from_str::<Value>(data) else {
                return Ok(true);
            };
            match ev["type"].as_str().unwrap_or("") {
                "message_start" => {
                    let u = &ev["message"]["usage"];
                    usage.input = u["input_tokens"].as_u64().unwrap_or(0)
                        + u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                    usage.cached = u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                }
                "content_block_start" => {
                    let cb = &ev["content_block"];
                    parts.push(match cb["type"].as_str() {
                        Some("text") => {
                            Partial::Text(cb["text"].as_str().unwrap_or("").to_string())
                        }
                        Some("tool_use") => Partial::Tool {
                            id: cb["id"].as_str().unwrap_or("").to_string(),
                            name: cb["name"].as_str().unwrap_or("").to_string(),
                            json: String::new(),
                        },
                        _ => Partial::Skip,
                    });
                }
                "content_block_delta" => {
                    let d = &ev["delta"];
                    match (parts.last_mut(), d["type"].as_str()) {
                        (Some(Partial::Text(t)), Some("text_delta")) => {
                            let s = d["text"].as_str().unwrap_or("");
                            t.push_str(s);
                            on_event(StreamEvent::Text(s));
                        }
                        (Some(Partial::Tool { json, .. }), Some("input_json_delta")) => {
                            json.push_str(d["partial_json"].as_str().unwrap_or(""))
                        }
                        _ => {}
                    }
                }
                "message_delta" => {
                    stop_reason = ev["delta"]["stop_reason"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    usage.output = ev["usage"]["output_tokens"]
                        .as_u64()
                        .unwrap_or(usage.output);
                }
                "message_stop" => {
                    finished = true;
                    return Ok(false);
                }
                "error" => {
                    let msg = ev["error"]["message"].as_str().unwrap_or("stream error");
                    let kind = ev["error"]["type"].as_str().unwrap_or("");
                    return Err(if kind == "overloaded_error" {
                        ProviderError::Network(msg.into())
                    } else {
                        ProviderError::Protocol(msg.into())
                    });
                }
                _ => {}
            }
            Ok(true)
        })?;
        if !finished {
            return Err(ProviderError::Network(
                "stream ended before the message completed".into(),
            ));
        }

        let blocks: Vec<Block> = parts
            .into_iter()
            .filter_map(|p| match p {
                Partial::Text(t) if !t.is_empty() => Some(Block::Text { text: t }),
                Partial::Tool { id, name, json } => {
                    let args = if json.trim().is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&json).unwrap_or_else(|_| json!({}))
                    };
                    Some(Block::ToolCall { id, name, args })
                }
                _ => None,
            })
            .collect();
        Ok(Completion::assistant(
            blocks,
            usage,
            stop_reason == "max_tokens",
        ))
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let mut headers = self.ep.auth.headers(false)?;
        headers.push(("anthropic-version".into(), API_VERSION.into()));
        let v = get_json(
            &quick_agent(),
            &self.ep.url("/v1/models?limit=100"),
            &headers,
        )?;
        Ok(v["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let id = m["id"].as_str()?.to_string();
                Some(ModelInfo {
                    display_name: m["display_name"].as_str().unwrap_or(&id).to_string(),
                    id,
                    ..ModelInfo::default()
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests;
