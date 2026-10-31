//! OpenAI-compatible Chat Completions over SSE: xAI/Grok, OpenRouter, Ollama, vLLM, and friends.

use crate::http::{
    Endpoint, auth_rejected, data_ids, get_json, models_from_ids, post_json, quick_agent,
    stream_events,
};
use rig_core::message::{Block, Message, Role};
use rig_core::provider::*;
use serde_json::{Value, json};

pub struct ChatCompletions {
    ep: Endpoint,
}

impl ChatCompletions {
    pub(crate) fn new(ep: Endpoint) -> Self {
        Self { ep }
    }
}

fn to_messages(system: &str, messages: &[Message]) -> Vec<Value> {
    let mut out = vec![json!({"role": "system", "content": system})];
    for m in messages {
        match m.role {
            Role::User => {
                let text = m.text();
                let images: Vec<Value> = m
                    .blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::Image { mime, data } => Some(json!({"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{data}")}})),
                        _ => None,
                    })
                    .collect();
                if !images.is_empty() {
                    let mut parts = vec![json!({"type": "text", "text": text})];
                    parts.extend(images);
                    out.push(json!({"role": "user", "content": parts}));
                } else if !text.is_empty() {
                    out.push(json!({"role": "user", "content": text}));
                }
                for b in &m.blocks {
                    if let Block::ToolResult { id, output, .. } = b {
                        out.push(json!({"role": "tool", "tool_call_id": id, "content": output}));
                    }
                }
            }
            Role::Assistant => {
                let calls: Vec<Value> = m
                    .tool_calls()
                    .map(|(id, name, args)| json!({"id": id, "type": "function", "function": {"name": name, "arguments": args.to_string()}}))
                    .collect();
                let text = m.text();
                let mut msg = json!({"role": "assistant", "content": if text.is_empty() { Value::Null } else { json!(text) }});
                if !calls.is_empty() {
                    msg["tool_calls"] = json!(calls);
                }
                out.push(msg);
            }
        }
    }
    out
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    args: String,
}

impl Provider for ChatCompletions {
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
        let mut body = json!({
            "model": self.ep.model,
            "messages": to_messages(req.system, req.messages),
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        if !req.tools.is_empty() {
            body["tools"] = json!(req
                .tools
                .iter()
                .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}}))
                .collect::<Vec<_>>());
        }
        let mut headers = self.ep.auth.headers(false)?;
        headers.push(("Accept".into(), "text/event-stream".into()));
        let resp = post_json(
            &self.ep.http,
            &self.ep.url("/chat/completions"),
            &headers,
            &body,
        )
        .map_err(auth_rejected)?;

        let mut text = String::new();
        let mut calls: Vec<PartialCall> = Vec::new();
        let mut usage = Usage::default();
        let mut finish = String::new();
        stream_events(resp, req.cancel, |_, data| {
            if data.trim() == "[DONE]" {
                return Ok(false);
            }
            let Ok(ev) = serde_json::from_str::<Value>(data) else {
                return Ok(true);
            };
            if let Some(err) = ev.get("error") {
                return Err(ProviderError::Protocol(
                    err["message"]
                        .as_str()
                        .unwrap_or("stream error")
                        .to_string(),
                ));
            }
            if ev["usage"].is_object() {
                usage.input = ev["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
                usage.output = ev["usage"]["completion_tokens"].as_u64().unwrap_or(0);
                usage.cached = ev["usage"]["prompt_tokens_details"]["cached_tokens"]
                    .as_u64()
                    .unwrap_or(0);
            }
            let choice = &ev["choices"][0];
            let delta = &choice["delta"];
            if let Some(t) = delta["content"].as_str().filter(|t| !t.is_empty()) {
                text.push_str(t);
                on_event(StreamEvent::Text(t));
            }
            if let Some(r) = delta["reasoning_content"]
                .as_str()
                .or(delta["reasoning"].as_str())
                .filter(|r| !r.is_empty())
            {
                on_event(StreamEvent::Reasoning(r));
            }
            for tc in delta["tool_calls"].as_array().into_iter().flatten() {
                let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                if calls.len() <= idx {
                    calls.resize_with(idx + 1, PartialCall::default);
                }
                let c = &mut calls[idx];
                if let Some(id) = tc["id"].as_str() {
                    c.id = id.to_string();
                }
                if let Some(n) = tc["function"]["name"].as_str() {
                    c.name.push_str(n);
                }
                if let Some(a) = tc["function"]["arguments"].as_str() {
                    c.args.push_str(a);
                }
            }
            if let Some(f) = choice["finish_reason"].as_str() {
                finish = f.to_string();
            }
            Ok(true)
        })?;

        let mut blocks = Vec::new();
        if !text.is_empty() {
            blocks.push(Block::Text { text });
        }
        for (i, c) in calls
            .into_iter()
            .enumerate()
            .filter(|(_, c)| !c.name.is_empty())
        {
            let args = if c.args.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&c.args).unwrap_or_else(|_| json!({}))
            };
            let id = if c.id.is_empty() {
                format!("call_{i}")
            } else {
                c.id
            };
            blocks.push(Block::ToolCall {
                id,
                name: c.name,
                args,
            });
        }
        Ok(Completion::assistant(blocks, usage, finish == "length"))
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let v = get_json(
            &quick_agent(),
            &format!("{}/models", self.ep.base_url),
            &self.ep.auth.headers(false)?,
        )?;
        Ok(models_from_ids(data_ids(&v)))
    }
}

#[cfg(test)]
mod tests;
