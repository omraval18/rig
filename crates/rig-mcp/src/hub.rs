//! The configured MCP servers. A server starts the first time the model searches or calls a tool,
//! so an unused MCP setup costs nothing: no process, no tool schemas in the context.

use crate::client::{Client, RemoteTool};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Mutex;

/// One server from `[mcp.<name>]` in a config file.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSpec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl Default for ServerSpec {
    /// Enabled, like a config entry that leaves `enabled` out. A derived default would disable it.
    fn default() -> Self {
        ServerSpec {
            command: String::new(),
            args: Vec::new(),
            env: BTreeMap::new(),
            enabled: true,
        }
    }
}

struct Slot {
    name: String,
    spec: ServerSpec,
    client: Option<Client>,
    tools: Vec<RemoteTool>,
    /// Why the server could not start, if it failed. Not retried within one run.
    error: Option<String>,
    tried: bool,
}

/// A tool found by search, with what the model needs to call it.
#[derive(Debug, PartialEq)]
pub struct Hit {
    pub server: String,
    pub tool: RemoteTool,
}

pub struct Hub {
    slots: Mutex<Vec<Slot>>,
}

impl Hub {
    /// Enabled servers only, in name order. Nothing is started here.
    pub fn new(servers: &BTreeMap<String, ServerSpec>) -> Hub {
        let slots = servers
            .iter()
            .filter(|(_, s)| s.enabled)
            .map(|(name, spec)| Slot {
                name: name.clone(),
                spec: spec.clone(),
                client: None,
                tools: Vec::new(),
                error: None,
                tried: false,
            })
            .collect();
        Hub {
            slots: Mutex::new(slots),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.slots.lock().map(|s| s.is_empty()).unwrap_or(true)
    }

    /// Server names, for the context and `/mcp`. Never starts anything.
    pub fn names(&self) -> Vec<String> {
        self.slots
            .lock()
            .map(|s| s.iter().map(|x| x.name.clone()).collect())
            .unwrap_or_default()
    }

    /// One line per server for `/mcp`: its state and how many tools it offers once started.
    pub fn status(&self) -> Vec<String> {
        let Ok(slots) = self.slots.lock() else {
            return Vec::new();
        };
        slots
            .iter()
            .map(|s| {
                let state = match (&s.error, s.client.is_some(), s.tried) {
                    (Some(e), _, _) => format!("failed: {e}"),
                    (None, true, _) => format!("connected, {} tools", s.tools.len()),
                    (None, false, true) => "stopped".into(),
                    (None, false, false) => "not started (starts on first use)".into(),
                };
                format!("{}: {state}", s.name)
            })
            .collect()
    }

    /// Starts `slot` if it has not been tried yet.
    fn connect(slot: &mut Slot) {
        if slot.tried {
            return;
        }
        slot.tried = true;
        let env: Vec<(String, String)> = slot.spec.env.clone().into_iter().collect();
        let result = Client::spawn(&slot.spec.command, &slot.spec.args, &env).and_then(|mut c| {
            c.initialize()?;
            let tools = c.list_tools()?;
            Ok((c, tools))
        });
        match result {
            Ok((client, tools)) => {
                slot.client = Some(client);
                slot.tools = tools;
            }
            Err(e) => slot.error = Some(e),
        }
    }

    /// Tools whose name or description contains every word of `query`, best matches first. Starts
    /// every server that has not been tried yet, since any of them may hold the tool.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let Ok(mut slots) = self.slots.lock() else {
            return Vec::new();
        };
        for slot in slots.iter_mut() {
            Self::connect(slot);
        }
        let words: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(String::from)
            .collect();
        let mut scored: Vec<(usize, Hit)> = Vec::new();
        for slot in slots.iter() {
            for tool in &slot.tools {
                let hay = format!("{} {}", tool.name, tool.description).to_lowercase();
                if !words.iter().all(|w| hay.contains(w.as_str())) {
                    continue;
                }
                // A word in the name counts more than one in the description.
                let score = words
                    .iter()
                    .map(|w| {
                        if tool.name.to_lowercase().contains(w.as_str()) {
                            2
                        } else {
                            1
                        }
                    })
                    .sum();
                scored.push((
                    score,
                    Hit {
                        server: slot.name.clone(),
                        tool: tool.clone(),
                    },
                ));
            }
        }
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.tool.name.cmp(&b.1.tool.name))
        });
        scored.into_iter().take(limit).map(|(_, h)| h).collect()
    }

    /// Whether a named tool exists and is read-only, starting its server if needed. `None` when
    /// the server or tool is unknown or the server cannot start.
    pub fn lookup(&self, server: &str, tool: &str) -> Option<bool> {
        let mut slots = self.slots.lock().ok()?;
        let slot = slots.iter_mut().find(|s| s.name == server)?;
        Self::connect(slot);
        slot.tools
            .iter()
            .find(|t| t.name == tool)
            .map(|t| t.read_only)
    }

    /// Runs a tool on a server. The result is the server's text, and whether it reported an error.
    pub fn call(&self, server: &str, tool: &str, args: Value) -> Result<(String, bool), String> {
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| "the MCP servers are busy".to_string())?;
        let slot = slots
            .iter_mut()
            .find(|s| s.name == server)
            .ok_or_else(|| format!("no MCP server named '{server}'"))?;
        Self::connect(slot);
        if let Some(e) = &slot.error {
            return Err(format!("server '{server}' is not running: {e}"));
        }
        if !slot.tools.iter().any(|t| t.name == tool) {
            return Err(format!("server '{server}' has no tool named '{tool}'"));
        }
        let client = slot
            .client
            .as_mut()
            .ok_or_else(|| format!("server '{server}' is not running"))?;
        client.call_tool(tool, args)
    }
}
