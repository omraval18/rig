//! The two tools the model sees for MCP. Their tools' schemas are not in the context: the model
//! searches for what it needs with `mcp_search`, then runs it with `mcp_call`. That keeps the tool
//! list the same size however many servers and tools are configured.

use crate::hub::Hub;
use rig_core::provider::ToolSpec;
use rig_core::tool::{Access, Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};
use std::sync::Arc;

/// Most results one search returns.
const SEARCH_LIMIT: usize = 8;
/// Longest schema shown per tool in search results.
const MAX_SCHEMA_CHARS: usize = 1_200;

/// The tools to add to the registry: nothing when no server is configured.
pub fn tools(hub: Arc<Hub>) -> Vec<Box<dyn Tool>> {
    if hub.is_empty() {
        return Vec::new();
    }
    vec![
        Box::new(Search { hub: hub.clone() }),
        Box::new(Call { hub }),
    ]
}

/// A short line for the context: which servers exist, and how to reach their tools.
pub fn catalog(hub: &Hub) -> Option<String> {
    let names = hub.names();
    if names.is_empty() {
        return None;
    }
    Some(format!(
        "MCP servers: {}. Their tools are not loaded yet: use mcp_search with a few words about the task to find them, then mcp_call to run one.",
        names.join(", ")
    ))
}

struct Search {
    hub: Arc<Hub>,
}

impl Tool for Search {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "mcp_search".into(),
            description: "Find tools on the configured MCP servers. Returns the matching tools with their arguments, so you can call them with mcp_call. Starts the servers the first time it is used.".into(),
            parameters: json!({
                "type": "object",
                "properties": {"query": {"type": "string", "description": "A few words about what you need, such as 'create issue' or 'read file'."}},
                "required": ["query"]
            }),
        }
    }

    fn access(&self, _: &Value) -> Access {
        Access::Read
    }

    fn describe(&self, args: &Value) -> String {
        format!("search: {}", args["query"].as_str().unwrap_or(""))
    }

    fn run(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let Some(query) = args["query"]
            .as_str()
            .map(str::trim)
            .filter(|q| !q.is_empty())
        else {
            return ToolOutput::err("mcp_search needs a non-empty 'query'");
        };
        let hits = self.hub.search(query, SEARCH_LIMIT);
        if hits.is_empty() {
            return ToolOutput::ok(format!("No MCP tool matches '{query}'."));
        }
        let mut out = String::new();
        for h in hits {
            let schema = h.tool.schema.to_string();
            let schema: String = schema.chars().take(MAX_SCHEMA_CHARS).collect();
            out.push_str(&format!(
                "{} {}: {}\n  arguments: {}\n",
                h.server, h.tool.name, h.tool.description, schema
            ));
        }
        ToolOutput::ok(out.trim_end())
    }
}

struct Call {
    hub: Arc<Hub>,
}

impl Tool for Call {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "mcp_call".into(),
            description: "Run a tool found with mcp_search. Pass the server and tool names exactly as the search showed them, and the arguments as the search showed them.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server": {"type": "string"},
                    "tool": {"type": "string"},
                    "arguments": {"type": "object"}
                },
                "required": ["server", "tool"]
            }),
        }
    }

    fn access(&self, args: &Value) -> Access {
        let server = args["server"].as_str().unwrap_or("").to_string();
        let tool = args["tool"].as_str().unwrap_or("").to_string();
        // Unknown tools are not read-only; they will fail when run, and are gated as writes.
        let read_only = self.hub.lookup(&server, &tool).unwrap_or(false);
        Access::Mcp {
            server,
            tool,
            read_only,
        }
    }

    fn describe(&self, args: &Value) -> String {
        format!(
            "{}.{}",
            args["server"].as_str().unwrap_or("?"),
            args["tool"].as_str().unwrap_or("?")
        )
    }

    fn run(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let server = args["server"].as_str().unwrap_or("");
        let tool = args["tool"].as_str().unwrap_or("");
        let arguments = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
        match self.hub.call(server, tool, arguments) {
            Ok((text, false)) => ToolOutput::ok(text),
            Ok((text, true)) => ToolOutput::err(text),
            Err(e) => ToolOutput::err(e),
        }
    }
}
