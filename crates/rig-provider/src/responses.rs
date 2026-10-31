//! OpenAI Responses API over SSE. One adapter serves both the public API
//! (API key) and the ChatGPT-plan backend; they accept the same stateless request.
//!
//! The request is deliberately stateless (`store: false`): history is replayed on
//! every turn, with encrypted reasoning items so the model keeps its chain of thought.
//! Fields the ChatGPT backend rejects (`max_output_tokens`, `temperature`, ...) are never sent.

use crate::http::{
    Endpoint, auth_rejected, data_ids, get_json, models_from_ids, post_json, quick_agent,
    stream_events,
};
use rig_core::message::{Block, Message, Opaque, Role};
use rig_core::provider::Effort;
use rig_core::provider::*;
use serde_json::{Value, json};

/// Tag for [`Opaque`] reasoning items that only this wire format may replay.
const WIRE: &str = "openai-responses";

pub struct Responses {
    ep: Endpoint,
    /// ChatGPT-plan backend: adds `originator`/`session_id` headers.
    chatgpt_backend: bool,
    /// Offer the provider's hosted web search tool alongside our own tools.
    web_search: bool,
}

impl Responses {
    pub(crate) fn new(ep: Endpoint, chatgpt_backend: bool, web_search: bool) -> Self {
        Self {
            ep,
            chatgpt_backend,
            web_search,
        }
    }

    fn body(&self, req: &Request) -> Value {
        let mut body = json!({
            "model": self.ep.model,
            "instructions": req.system,
            "input": to_input(req.messages),
            "tools": req.tools.iter().map(|t| json!({
                "type": "function", "name": t.name, "description": t.description,
                "parameters": t.parameters, "strict": false,
            })).collect::<Vec<_>>(),
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "store": false,
            "stream": true,
            "include": ["reasoning.encrypted_content"],
            "prompt_cache_key": req.session_id,
        });
        if self.web_search {
            // The provider runs the search itself; its results come back inside the reply.
            if let Some(tools) = body["tools"].as_array_mut() {
                tools.push(json!({"type": "web_search"}));
            }
        }
        if let Some(e) = req.effort {
            body["reasoning"] = json!({"effort": e.as_str(), "summary": "auto"});
        }
        body
    }

    fn headers(
        &self,
        req: &Request,
        force_refresh: bool,
    ) -> Result<Vec<(String, String)>, ProviderError> {
        let mut h = self.ep.auth.headers(force_refresh)?;
        h.push(("Accept".into(), "text/event-stream".into()));
        if self.chatgpt_backend {
            h.push(("originator".into(), "rig".into()));
            h.push((
                "User-Agent".into(),
                format!("rig/{}", env!("CARGO_PKG_VERSION")),
            ));
            h.push(("session_id".into(), req.session_id.to_string()));
        }
        Ok(h)
    }
}

fn to_input(messages: &[Message]) -> Vec<Value> {
    let mut input = Vec::new();
    for m in messages {
        let mut text_parts: Vec<Value> = Vec::new();
        let flush = |parts: &mut Vec<Value>, input: &mut Vec<Value>, role: Role| {
            if !parts.is_empty() {
                input.push(json!({"role": if role == Role::User { "user" } else { "assistant" }, "content": std::mem::take(parts)}));
            }
        };
        for b in &m.blocks {
            match (m.role, b) {
                (Role::User, Block::Text { text }) => text_parts.push(json!({"type": "input_text", "text": text})),
                (Role::Assistant, Block::Text { text }) => text_parts.push(json!({"type": "output_text", "text": text})),
                (_, Block::ToolResult { id, output, .. }) => {
                    flush(&mut text_parts, &mut input, m.role);
                    input.push(json!({"type": "function_call_output", "call_id": id, "output": output}));
                }
                (_, Block::ToolCall { id, name, args }) => {
                    flush(&mut text_parts, &mut input, m.role);
                    input.push(json!({"type": "function_call", "call_id": id, "name": name, "arguments": args.to_string()}));
                }
                (_, Block::Reasoning { raw: Some(o), .. }) if o.provider == WIRE => {
                    flush(&mut text_parts, &mut input, m.role);
                    input.push(o.data.clone());
                }
                (_, Block::Reasoning { .. }) => {}
                (_, Block::Image { mime, data }) => text_parts.push(json!({"type": "input_image", "image_url": format!("data:{mime};base64,{data}")})),
            }
        }
        flush(&mut text_parts, &mut input, m.role);
    }
    input
}

fn parse_effort(v: &Value) -> Option<Effort> {
    Effort::parse(v.as_str()?)
}

impl Provider for Responses {
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
        let body = self.body(req);
        let url = self.ep.url("/responses");
        let mut force_refresh = false;
        let resp = loop {
            match post_json(
                &self.ep.http,
                &url,
                &self.headers(req, force_refresh)?,
                &body,
            ) {
                Err(ProviderError::Http { status: 401, .. })
                    if self.ep.auth.refreshable() && !force_refresh =>
                {
                    force_refresh = true
                }
                other => break other.map_err(auth_rejected)?,
            }
        };

        let mut blocks: Vec<Block> = Vec::new();
        let mut usage = Usage::default();
        let mut finished = false;
        let mut hit_limit = false;
        stream_events(resp, req.cancel, |_, data| {
            let Ok(ev) = serde_json::from_str::<Value>(data) else {
                return Ok(true);
            };
            match ev["type"].as_str().unwrap_or("") {
                "response.output_text.delta" => {
                    on_event(StreamEvent::Text(ev["delta"].as_str().unwrap_or("")))
                }
                "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                    on_event(StreamEvent::Reasoning(ev["delta"].as_str().unwrap_or("")))
                }
                "response.output_item.done" => {
                    if let Some(b) = item_to_block(&ev["item"]) {
                        blocks.push(b);
                    }
                }
                "response.completed" | "response.incomplete" => {
                    let r = &ev["response"];
                    usage = Usage {
                        input: r["usage"]["input_tokens"].as_u64().unwrap_or(0),
                        output: r["usage"]["output_tokens"].as_u64().unwrap_or(0),
                        cached: r["usage"]["input_tokens_details"]["cached_tokens"]
                            .as_u64()
                            .unwrap_or(0),
                        cache_write: 0,
                    };
                    hit_limit =
                        r["incomplete_details"]["reason"].as_str() == Some("max_output_tokens");
                    finished = true;
                    return Ok(false);
                }
                "response.failed" => {
                    let msg = ev["response"]["error"]["message"]
                        .as_str()
                        .unwrap_or("response failed");
                    return Err(ProviderError::Protocol(msg.to_string()));
                }
                "error" => {
                    let msg = ev["message"]
                        .as_str()
                        .or(ev["error"]["message"].as_str())
                        .unwrap_or("stream error");
                    return Err(ProviderError::Protocol(msg.to_string()));
                }
                _ => {}
            }
            Ok(true)
        })?;
        if !finished {
            return Err(ProviderError::Network(
                "stream ended before the response completed".into(),
            ));
        }
        Ok(Completion::assistant(blocks, usage, hit_limit))
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let mut headers = self.ep.auth.headers(false)?;
        let quick = quick_agent();
        if !self.chatgpt_backend {
            let v = get_json(&quick, &self.ep.url("/models"), &headers)?;
            let mut ids = data_ids(&v);
            ids.retain(|id| id.starts_with("gpt") || id.starts_with("o") || id.contains("codex"));
            return Ok(models_from_ids(ids));
        }
        headers.push(("originator".into(), "rig".into()));
        headers.push((
            "User-Agent".into(),
            format!("rig/{}", env!("CARGO_PKG_VERSION")),
        ));
        headers.push(("Accept".into(), "application/json".into()));
        let v = get_json(
            &quick,
            &self.ep.url("/models?client_version=0.165.0"),
            &headers,
        )?;
        let mut models: Vec<(i64, ModelInfo)> = v["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let id = m["slug"].as_str()?.to_string();
                Some((
                    m["priority"].as_i64().unwrap_or(999),
                    ModelInfo {
                        display_name: m["display_name"].as_str().unwrap_or(&id).to_string(),
                        description: m["description"].as_str().unwrap_or("").to_string(),
                        context_window: m["context_window"].as_u64(),
                        efforts: m["supported_reasoning_levels"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|l| parse_effort(&l["effort"]))
                            .collect(),
                        default_effort: parse_effort(&m["default_reasoning_level"]),
                        hidden: m["visibility"].as_str() == Some("hide"),
                        id,
                    },
                ))
            })
            .collect();
        models.sort_by_key(|(p, _)| *p);
        Ok(models.into_iter().map(|(_, m)| m).collect())
    }
}

fn item_to_block(item: &Value) -> Option<Block> {
    match item["type"].as_str()? {
        "message" => {
            let text: String = item["content"]
                .as_array()?
                .iter()
                .filter_map(|c| c["text"].as_str().or(c["refusal"].as_str()))
                .collect();
            (!text.is_empty()).then_some(Block::Text { text })
        }
        "function_call" => Some(Block::ToolCall {
            id: item["call_id"].as_str()?.to_string(),
            name: item["name"].as_str()?.to_string(),
            args: serde_json::from_str(item["arguments"].as_str().unwrap_or("{}"))
                .unwrap_or_else(|_| json!({})),
        }),
        "reasoning" => {
            let text = item["summary"]
                .as_array()
                .map(|s| {
                    s.iter()
                        .filter_map(|p| p["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            Some(Block::Reasoning {
                text,
                raw: Some(Opaque {
                    provider: WIRE.into(),
                    data: item.clone(),
                }),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
