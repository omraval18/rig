//! What each event from the agent worker does to the screen.

use super::format::tool_label;
use super::{Part, PendingApproval, RunTool, Tui};
use crate::runtime::Ev;
use crate::view::{self, ApprovalView};
use rig_core::permission::Approval;
use std::sync::mpsc::Sender;
use std::time::Instant;

/// Rows of a tool's result summary shown before the rest is cut.
const SUMMARY_ROWS: usize = 12;

impl Tui {
    pub(super) fn on_ev(&mut self, ev: Ev) {
        match ev {
            Ev::Text(delta) => self.on_text(&delta),
            Ev::Reasoning(delta) => self.on_reasoning(&delta),
            Ev::ToolStart { name, desc } => {
                self.flush_partial();
                self.turn.tools.push_back(RunTool {
                    name,
                    desc,
                    at: Instant::now(),
                });
                self.dirty = true;
            }
            Ev::ToolEnd { error, summary } => self.on_tool_end(error, summary),
            Ev::Usage(usage) => self.turn.usage += usage,
            Ev::Context(percent) => {
                self.info.ctx_pct = percent;
                self.dirty = true;
            }
            Ev::Retrying(message) => self.notice(message),
            Ev::Compacting(on) => {
                self.turn.compacting = on;
                self.dirty = true;
            }
            Ev::Approval {
                name,
                desc,
                reason,
                remember,
                reply,
            } => self.ask_approval(&name, desc, reason, remember, reply),
            Ev::Done { agent, result } => self.finish_job(agent, result),
            Ev::Question {
                question,
                options,
                reply,
            } => self.open_question(question, options, reply),
            Ev::Models {
                provider,
                seq,
                models,
            } => self.on_models(&provider, seq, models),
            Ev::Catalog => self.refresh_provider_picker(),
            Ev::Probed { url, key, result } => self.endpoint_probed(&url, &key, result),
            Ev::Notice(text) => self.say(&text),
            Ev::ImageReady(result) => {
                self.prompt.pending_images = self.prompt.pending_images.saturating_sub(1);
                match result {
                    Ok((block, name)) => self.attach(block, &name),
                    Err(error) => self.notice(error),
                }
            }
            Ev::RolledBack => self.turn.rolled_back = true,
            Ev::LoginDone(Ok(message)) => {
                self.say(&message);
                self.connected("chatgpt");
            }
            Ev::LoginDone(Err(e)) => self.error(&e),
        }
    }

    fn on_text(&mut self, delta: &str) {
        self.turn.reply.push_str(delta);
        self.turn.partial.push_str(delta);
        for line in take_lines(&mut self.turn.partial) {
            let styled = self.display.styler.line(&line);
            self.print_part(Part::Reply, &styled);
        }
        self.dirty = true;
    }

    fn on_reasoning(&mut self, delta: &str) {
        if !self.show_reasoning {
            return;
        }
        self.turn.reasoning_buf.push_str(delta);
        for line in take_lines(&mut self.turn.reasoning_buf) {
            let dimmed = self.dim(&format!("  {}", line.trim_end()));
            self.print_part(Part::Think, &dimmed);
        }
    }

    fn on_tool_end(&mut self, error: Option<String>, summary: Option<String>) {
        // Ends arrive in the order the calls started.
        let Some(tool) = self.turn.tools.pop_front() else {
            return;
        };
        let mut block = self.tool_row(Some(error.is_none()), &tool.name, &tool.desc, None);
        let room = self.display.width.saturating_sub(5);
        if let Some(e) = error {
            let line = format!("  ⎿ {}", view::truncate(&self.shorten(&e), room));
            block.push_str(&format!("\n{}", self.red(&line)));
        } else if let Some(summary) = summary {
            for (i, line) in summary.lines().take(SUMMARY_ROWS).enumerate() {
                let elbow = if i == 0 { "⎿" } else { " " };
                let row = format!("  {elbow} {}", view::truncate(line, room));
                block.push_str(&format!("\n{}", self.dim(&row)));
            }
        }
        self.print_part(Part::Tool, &block);
    }

    fn ask_approval(
        &mut self,
        name: &str,
        detail: String,
        reason: String,
        remember: Option<String>,
        reply: Sender<Approval>,
    ) {
        self.flush_partial();
        let always = remember
            .as_deref()
            .map_or("Always".to_string(), |rule| format!("Always: {rule}"));
        self.prompt.approval = Some(PendingApproval {
            view: ApprovalView {
                title: format!("Allow {}?", tool_label(name)),
                detail,
                reason,
                always,
                selected: 0,
            },
            reply,
            remember,
        });
        self.prompt.completion = None;
        self.dirty = true;
    }
}

/// Remove and return every complete line (without its newline) from `buf`.
fn take_lines(buf: &mut String) -> Vec<String> {
    let mut lines = Vec::new();
    while let Some(i) = buf.find('\n') {
        let line: String = buf.drain(..=i).collect();
        lines.push(line.trim_end_matches('\n').to_string());
    }
    lines
}
