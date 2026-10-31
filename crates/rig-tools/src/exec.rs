use crate::util::{bool_arg, str_arg, usize_arg};
use rig_core::provider::ToolSpec;
use rig_core::text::head_tail;
use rig_core::tool::{Access, Risk, Tool, ToolCtx, ToolOutput};
use serde_json::{Value, json};
use std::fs::File;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct RunCommand;

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const RESULT_BUDGET: usize = 32 * 1024;
/// Stop buffering beyond this; the rest is drained and dropped.
const CAPTURE_CAP: usize = 16 * 1024 * 1024;

impl Tool for RunCommand {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "run_command".into(),
            description: "Run a shell command. Every call starts a NEW shell in the session working directory: `cd`, exports and activated environments do not persist, so chain setup and use in one command. Commands that wait for input hang; use non-interactive flags. Output is capped at 32 KiB (head and tail are kept). Commands are killed at the timeout; raise `timeout_secs` for long builds. Never run dev servers or watchers in the foreground: set background=true, which returns a PID and log path immediately. Use read_file, not cat/head/tail, to read files. Never push or rewrite published git history unless the user asked, and never change the configured git identity.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "purpose": {"type": "string", "description": "REQUIRED. Sentence-cased summary of the command, 3-8 words."},
                    "command": {"type": "string", "description": "The command to execute."},
                    "timeout_secs": {"type": "number", "description": "Timeout in seconds (default 60)."},
                    "risk": {"type": "string", "enum": ["low", "medium", "high"], "description": "REQUIRED. low: read-only or trivially reversible. medium: reversible workspace changes (installs, builds, local commits). high: destructive, irreversible, or visible to others (push, delete, publish, sudo)."},
                    "background": {"type": "boolean", "description": "Run in the background and return the PID and log path immediately."}
                },
                "required": ["purpose", "command", "risk"]
            }),
        }
    }

    fn access(&self, args: &Value) -> Access {
        Access::Exec {
            risk: str_arg(args, "risk")
                .and_then(Risk::parse)
                .unwrap_or(Risk::High),
            command: str_arg(args, "command").unwrap_or("").to_string(),
        }
    }

    fn describe(&self, args: &Value) -> String {
        let summary = str_arg(args, "purpose").unwrap_or("Run command");
        let command = str_arg(args, "command").unwrap_or("");
        let first = command.lines().next().unwrap_or("");
        let shown: String = first.chars().take(100).collect();
        format!(
            "{summary}  $ {shown}{}",
            if command.len() > shown.len() {
                " …"
            } else {
                ""
            }
        )
    }

    fn summarize(&self, _: &Value, out: &ToolOutput) -> Option<String> {
        let (body, code) = match out.text.rsplit_once("\n\n[Process exited with code ") {
            Some((body, tail)) => (body, tail.trim_end_matches(']').to_string()),
            None => (out.text.as_str(), String::new()),
        };
        let line = if out.is_error {
            rig_core::tool::error_line(body)
        } else {
            rig_core::tool::last_meaningful_line(body)
        };
        match (out.is_error, code.as_str()) {
            (true, "" | "signal") => line,
            (true, c) => Some(format!(
                "exit {c}{}",
                line.map(|l| format!(" · {l}")).unwrap_or_default()
            )),
            (false, _) => line,
        }
    }

    fn run(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let Some(command) = str_arg(args, "command") else {
            return ToolOutput::err("command is required");
        };
        let timeout = Duration::from_secs(
            usize_arg(args, "timeout_secs")
                .map(|n| n as u64)
                .unwrap_or(DEFAULT_TIMEOUT_SECS)
                .max(1),
        );
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(command)
            .current_dir(&ctx.cwd)
            .stdin(Stdio::null())
            .env("AI_AGENT", "rig")
            .process_group(0);

        if bool_arg(args, "background") {
            return spawn_detached(cmd);
        }

        let (mut reader, writer) = match std::io::pipe() {
            Ok(p) => p,
            Err(e) => return ToolOutput::err(format!("cannot create pipe: {e}")),
        };
        let writer2 = match writer.try_clone() {
            Ok(w) => w,
            Err(e) => return ToolOutput::err(format!("cannot create pipe: {e}")),
        };
        cmd.stdout(writer).stderr(writer2);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return ToolOutput::err(format!("cannot start shell: {e}")),
        };
        // The Command still owns the parent's copies of the write end; drop it
        // so the reader sees EOF when the child exits.
        drop(cmd);

        let collector = thread::spawn(move || {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 16 * 1024];
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                if buf.len() < CAPTURE_CAP {
                    buf.extend_from_slice(&chunk[..n]);
                }
            }
            buf
        });

        let deadline = Instant::now() + timeout;
        let mut nap = Duration::from_millis(1);
        let mut cancelled = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) if ctx.cancelled() || Instant::now() >= deadline => {
                    cancelled = ctx.cancelled();
                    let _ = Command::new("kill")
                        .args(["-KILL", &format!("-{}", child.id())])
                        .status();
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => {
                    thread::sleep(nap);
                    nap = (nap * 2).min(Duration::from_millis(25));
                }
                Err(e) => return ToolOutput::err(format!("wait failed: {e}")),
            }
        };

        let bytes = collector.join().unwrap_or_default();
        let mut out = head_tail(&String::from_utf8_lossy(&bytes), RESULT_BUDGET);
        match status {
            Some(s) => {
                let code = s
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".into());
                out.push_str(&format!("\n\n[Process exited with code {code}]"));
                ToolOutput {
                    text: out,
                    is_error: !s.success(),
                }
            }
            None if cancelled => {
                out.push_str("\n\n[Command cancelled by the user and killed]");
                ToolOutput::err(out)
            }
            None => {
                out.push_str(&format!(
                    "\n\n[Command timed out after {}s and was killed]",
                    timeout.as_secs()
                ));
                ToolOutput::err(out)
            }
        }
    }
}

fn spawn_detached(mut cmd: Command) -> ToolOutput {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let log = std::env::temp_dir().join(format!("rig-job-{nanos}.log"));
    let file = match File::create(&log) {
        Ok(f) => f,
        Err(e) => return ToolOutput::err(format!("cannot create log file: {e}")),
    };
    let file2 = match file.try_clone() {
        Ok(f) => f,
        Err(e) => return ToolOutput::err(format!("cannot create log file: {e}")),
    };
    cmd.stdout(file).stderr(file2);
    match cmd.spawn() {
        Ok(child) => ToolOutput::ok(format!(
            "Started in the background. PID {}. Output: {}\nCheck the log with Read when you need it; do not poll.",
            child.id(),
            log.display()
        )),
        Err(e) => ToolOutput::err(format!("cannot start shell: {e}")),
    }
}

#[cfg(test)]
mod tests;
