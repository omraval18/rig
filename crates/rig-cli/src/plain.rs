//! Plain output for `rig ask`: reply on stdout, progress on stderr. Implements the agent's `Ui`.

use rig_core::agent::Ui;
use rig_core::message::Message;
use rig_core::provider::{ProviderError, Usage};
use rig_core::session::Session;
use rig_core::tool::ToolOutput;
use std::io::{self, IsTerminal, Write};

pub struct PlainUi {
    color: bool,
    /// Collect the reply instead of printing it (`--json`).
    pub capture: Option<String>,
    pub session: Option<Session>,
    pub usage: Usage,
    mid_line: bool,
    pending: std::collections::VecDeque<(String, String)>,
}

impl PlainUi {
    pub fn new() -> Self {
        let color = io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
        PlainUi {
            color,
            capture: None,
            session: None,
            usage: Usage::default(),
            mid_line: false,
            pending: Default::default(),
        }
    }

    fn dim(&self, s: &str) -> String {
        if self.color {
            format!("\x1b[2m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    fn red(&self, s: &str) -> String {
        if self.color {
            format!("\x1b[31m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    fn line_break(&mut self) {
        if self.mid_line && self.capture.is_none() {
            println!();
        }
        self.mid_line = false;
    }

    pub fn finish(&mut self) {
        self.line_break();
    }

    pub fn error(&self, s: &str) {
        eprintln!("{}", self.red(&format!("error: {s}")));
    }
}

impl Ui for PlainUi {
    fn text(&mut self, delta: &str) {
        if let Some(buf) = &mut self.capture {
            buf.push_str(delta);
            return;
        }
        print!("{delta}");
        let _ = io::stdout().flush();
        self.mid_line = !delta.ends_with('\n');
    }

    fn tool_start(&mut self, name: &str, description: &str) {
        self.pending
            .push_back((name.to_string(), description.to_string()));
    }

    fn tool_end(&mut self, _name: &str, output: &ToolOutput, summary: Option<&str>) {
        use crate::style::{self, GREEN, RED};
        self.line_break();
        let (name, desc) = self.pending.pop_front().unwrap_or_default();
        let dot = if output.is_error {
            style::paint(self.color, RED, "●")
        } else {
            style::paint(self.color, GREEN, "●")
        };
        eprintln!(
            "{dot} {}  {}",
            style::paint(self.color, style::BOLD, &name),
            self.dim(&desc)
        );
        let line = summary
            .map(String::from)
            .or_else(|| rig_core::tool::last_meaningful_line(&output.text));
        if output.is_error {
            eprintln!(
                "{}",
                style::paint(
                    self.color,
                    RED,
                    &format!("  ⎿ {}", line.unwrap_or_else(|| "failed".into()))
                )
            );
        } else if let Some(s) = summary {
            for (i, l) in s.lines().enumerate() {
                eprintln!(
                    "{}",
                    self.dim(&format!("  {} {l}", if i == 0 { "⎿" } else { " " }))
                );
            }
        }
    }

    fn usage(&mut self, u: &Usage) {
        self.usage += *u;
    }

    fn retrying(&mut self, attempt: u32, error: &ProviderError) {
        self.line_break();
        eprintln!("{}", self.dim(&format!("retrying ({attempt}/3): {error}")));
    }

    fn message(&mut self, message: &Message) {
        if let Some(s) = &mut self.session {
            let _ = s.append(message);
        }
    }

    fn checkpoint(&mut self, split: usize, checkpoint: &Message) {
        if let Some(s) = &mut self.session {
            let _ = s.append_checkpoint(split, checkpoint);
        }
    }
}
