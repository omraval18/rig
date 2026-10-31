//! Running the tool calls of one assistant turn: permissions first, then batches that respect read/write ordering.

use crate::message::Block;
use crate::permission::{Approval, Decision};
use crate::tool::{Access, ToolOutput};
use std::thread;

use super::{Agent, Ui};
use crate::text::head_tail;

/// Hard cap on a single tool result, as a guard against blowing the context.
const MAX_RESULT_BYTES: usize = 96 * 1024;

/// One requested call, after its permission was decided.
struct Planned {
    id: String,
    name: String,
    args: serde_json::Value,
    /// Read-only calls may run alongside their neighbours; an edit or command runs alone.
    concurrent: bool,
    /// Set when the call will not run; this is what the model is told instead.
    blocked: Option<ToolOutput>,
}

const CANCELLED: &str = "cancelled by the user before it ran";

impl Agent {
    /// Decide permissions up front (sequentially, since approvals prompt), then
    /// execute in batches that respect read/write ordering.
    pub(super) fn run_calls(
        &self,
        calls: Vec<(String, String, serde_json::Value)>,
        ui: &mut dyn Ui,
    ) -> Vec<Block> {
        let planned: Vec<Planned> = calls
            .into_iter()
            .map(|(id, name, args)| self.plan(id, name, args, ui))
            .collect();
        let mut outputs = Vec::with_capacity(planned.len());
        let mut start = 0;
        while start < planned.len() {
            let end = batch_end(&planned, start);
            outputs.extend(self.run_batch(&planned[start..end], ui));
            start = end;
        }
        planned
            .into_iter()
            .zip(outputs)
            .map(|(call, output)| Block::ToolResult {
                id: call.id,
                output: head_tail(&output.text, MAX_RESULT_BYTES),
                is_error: output.is_error,
            })
            .collect()
    }

    fn plan(&self, id: String, name: String, args: serde_json::Value, ui: &mut dyn Ui) -> Planned {
        let (concurrent, blocked) = match self.tools.get(&name) {
            None => (
                false,
                Some(ToolOutput::err(format!("unknown tool '{name}'"))),
            ),
            Some(_) if self.cancelled() => (false, Some(ToolOutput::err(CANCELLED))),
            Some(tool) => {
                let access = tool.access(&args);
                let desc = tool.describe(&args);
                let blocked = match &access {
                    Access::Ask { question, options } => {
                        Some(ask_output(ui.ask(question, options)))
                    }
                    _ => self.gate(&name, &desc, &access, ui),
                };
                (access.is_concurrent(), blocked)
            }
        };
        Planned {
            id,
            name,
            args,
            concurrent,
            blocked,
        }
    }

    /// Run one batch on scoped threads and report each call to the front end.
    fn run_batch(&self, batch: &[Planned], ui: &mut dyn Ui) -> Vec<ToolOutput> {
        // Every call announces itself, denied ones included, so a front end can pair each end with its start.
        for call in batch {
            let desc = self
                .tools
                .get(&call.name)
                .map(|t| t.describe(&call.args))
                .unwrap_or_default();
            ui.tool_start(&call.name, &desc);
        }
        let outputs: Vec<ToolOutput> = thread::scope(|scope| {
            // Collected on purpose: every call must be spawned before the first join.
            #[allow(clippy::needless_collect)]
            let handles: Vec<_> = batch
                .iter()
                .map(|call| scope.spawn(move || self.run_call(call)))
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .unwrap_or_else(|_| ToolOutput::err("tool panicked"))
                })
                .collect()
        });
        for (call, output) in batch.iter().zip(&outputs) {
            let summary = self
                .tools
                .get(&call.name)
                .and_then(|t| t.summarize(&call.args, output));
            ui.tool_end(&call.name, output, summary.as_deref());
        }
        outputs
    }

    fn run_call(&self, call: &Planned) -> ToolOutput {
        if let Some(blocked) = &call.blocked {
            return blocked.clone();
        }
        if self.cancelled() {
            return ToolOutput::err(CANCELLED);
        }
        match self.tools.get(&call.name) {
            Some(tool) => tool.run(&call.args, &self.ctx),
            None => ToolOutput::err("unknown tool"),
        }
    }

    /// `None` = go ahead; `Some(result)` = the call is blocked and this is what the model is told.
    fn gate(&self, name: &str, desc: &str, access: &Access, ui: &mut dyn Ui) -> Option<ToolOutput> {
        match self.policy.decide(access) {
            Decision::Allow => None,
            Decision::Deny(why) => Some(ToolOutput::err(format!("denied: {why}"))),
            Decision::Ask(why) => match ui.approve(
                name,
                desc,
                &why,
                crate::permission::rule_for(access).as_deref(),
            ) {
                Approval::Once => None,
                Approval::Always => {
                    self.policy.remember(access);
                    None
                }
                Approval::No => Some(ToolOutput::err(format!(
                    "not run: {why}. The user did not approve it; choose a safer approach or ask them."
                ))),
            },
        }
    }
}

/// What the model is told after a question. An answer is returned as the tool result.
fn ask_output(answer: Option<String>) -> ToolOutput {
    match answer {
        Some(a) => ToolOutput::ok(format!("The user answered: {a}")),
        None => ToolOutput::err(
            "No answer: the user did not reply, or nobody is available to ask. Decide for yourself and say which assumption you made.",
        ),
    }
}

/// A run of concurrent calls shares a batch; anything else is a batch of one.
fn batch_end(planned: &[Planned], start: usize) -> usize {
    if !planned[start].concurrent {
        return start + 1;
    }
    let run = planned[start..].iter().take_while(|p| p.concurrent).count();
    start + run
}
