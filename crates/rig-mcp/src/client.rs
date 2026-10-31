//! One MCP server over stdio: newline-delimited JSON-RPC 2.0, as the MCP spec describes it.
//! Server messages are read on a thread and handed over a channel, so a request can time out
//! instead of blocking the agent forever.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

/// The protocol revision we speak. Servers that support it answer `initialize` with it.
const PROTOCOL: &str = "2024-11-05";
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// Most tools kept from one server. More than this is almost certainly a broken server.
const MAX_TOOLS: usize = 500;

/// A tool as a server describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
    /// The server's `readOnlyHint` annotation. Absent means "not known to be read-only".
    pub read_only: bool,
}

impl RemoteTool {
    fn parse(v: &Value) -> Option<RemoteTool> {
        let name = v["name"].as_str()?.to_string();
        Some(RemoteTool {
            name,
            description: v["description"].as_str().unwrap_or("").to_string(),
            schema: v
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object"})),
            read_only: v["annotations"]["readOnlyHint"].as_bool().unwrap_or(false),
        })
    }
}

pub struct Client {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Value>,
    next_id: u64,
}

impl Client {
    /// Starts the server. Its stderr is discarded so it cannot draw over the UI.
    pub fn spawn(
        command: &str,
        args: &[String],
        env: &[(String, String)],
    ) -> Result<Client, String> {
        let mut child = Command::new(command)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot start {command}: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin for the server")?;
        let stdout = child.stdout.take().ok_or("no stdout from the server")?;
        let (tx, messages) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(msg) = serde_json::from_str::<Value>(&line)
                    && tx.send(msg).is_err()
                {
                    break;
                }
            }
        });
        Ok(Client {
            child,
            stdin,
            messages,
            next_id: 1,
        })
    }

    /// Sends a request and waits for its answer. Other messages (notifications, log lines) are
    /// skipped.
    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_default();
            let msg = self.messages.recv_timeout(left).map_err(|e| match e {
                RecvTimeoutError::Timeout => format!("the server did not answer {method} in time"),
                RecvTimeoutError::Disconnected => "the server exited".to_string(),
            })?;
            if msg.get("id") != Some(&json!(id)) {
                continue;
            }
            if let Some(err) = msg.get("error") {
                return Err(err["message"]
                    .as_str()
                    .unwrap_or("the server reported an error")
                    .to_string());
            }
            return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    fn send(&mut self, msg: &Value) -> Result<(), String> {
        let mut line = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("cannot write to the server: {e}"))
    }

    /// The handshake every session starts with.
    pub fn initialize(&mut self) -> Result<(), String> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": {"name": "rig", "version": env!("CARGO_PKG_VERSION")},
            }),
            CONNECT_TIMEOUT,
        )?;
        self.notify("notifications/initialized", json!({}))
    }

    /// Every tool the server offers, following pagination.
    pub fn list_tools(&mut self) -> Result<Vec<RemoteTool>, String> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let page = self.request("tools/list", params, CONNECT_TIMEOUT)?;
            out.extend(
                page["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(RemoteTool::parse),
            );
            cursor = page["nextCursor"].as_str().map(String::from);
            if cursor.is_none() || out.len() >= MAX_TOOLS {
                break;
            }
        }
        out.truncate(MAX_TOOLS);
        Ok(out)
    }

    /// Runs a tool. Returns its text output, and whether the server flagged it as an error.
    pub fn call_tool(&mut self, name: &str, args: Value) -> Result<(String, bool), String> {
        let r = self.request(
            "tools/call",
            json!({"name": name, "arguments": args}),
            CALL_TIMEOUT,
        )?;
        let text = r["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        Ok((text, r["isError"].as_bool().unwrap_or(false)))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
