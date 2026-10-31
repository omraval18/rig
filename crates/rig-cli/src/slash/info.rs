//! `/status` and `/stats`.

use crate::desktop;
use crate::tui::{Tui, fmt_tokens};
use rig_core::provider::Effort;
use rig_core::sync::MutexExt;

impl Tui {
    pub(super) fn cmd_stats(&mut self) {
        let t = self.stats.total;
        let id = self.session.locked().id.clone();
        let ctx = self.info.ctx_pct.map_or("unknown".into(), |p| {
            format!("{p}% of {}", fmt_tokens(self.info.context_window))
        });
        let s = &self.stats;
        let cost = if s.plan_turns > 0 && s.cost == 0.0 && s.unpriced_turns == 0 {
            "included in your ChatGPT plan".to_string()
        } else {
            let mut c = format!("≈ ${:.4} (list prices)", s.cost);
            if s.unpriced_turns > 0 {
                c.push_str(&format!(", {} turn(s) without a price", s.unpriced_turns));
            }
            if s.plan_turns > 0 {
                c.push_str(&format!(
                    ", {} ChatGPT plan turn(s) not counted",
                    s.plan_turns
                ));
            }
            c
        };
        self.say(&format!(
            "turns {} · {} in · {} out · {} cached\ncost {cost}\ncontext {ctx} · session {id}",
            s.turns,
            fmt_tokens(t.input),
            fmt_tokens(t.output),
            fmt_tokens(t.cached)
        ));
    }

    pub(super) fn cmd_status(&mut self) {
        let s = self.session.locked();
        let (id, path) = (s.id.clone(), s.path.display().to_string());
        drop(s);
        let effort = self.info.effort.map_or("none", Effort::as_str);
        let access = self.policy.autonomy();
        let row =
            |label: &str, value: String| format!("{} {value}", self.dim(&format!("{label:<12}")));
        let lines = [
            row(
                "profile",
                format!(
                    "{} {}",
                    self.app.paths.profile,
                    self.dim(&format!("({})", self.app.paths.root.display()))
                ),
            ),
            row(
                "model",
                crate::style::paint(
                    self.display.color,
                    crate::style::CYAN,
                    &format!("{}/{}", self.info.spec.name, self.info.model),
                ),
            ),
            row(
                "effort",
                crate::style::paint(self.display.color, crate::style::VIOLET, effort),
            ),
            row(
                "permissions",
                format!(
                    "{} {}",
                    crate::style::paint(
                        self.display.color,
                        crate::style::permission(access),
                        access.as_str()
                    ),
                    self.dim(&format!("({})", access.describe()))
                ),
            ),
            row(
                "context",
                format!(
                    "{} window, compacts at {}%",
                    fmt_tokens(self.info.context_window),
                    self.app.auto_compact_percent()
                ),
            ),
            row(
                "workspace",
                format!(
                    "{} ({} paths indexed{})",
                    self.app.cwd.display(),
                    self.index.len(),
                    if self.index.truncated() {
                        ", truncated"
                    } else {
                        ""
                    }
                ),
            ),
            row("skills", self.skills.len().to_string()),
            row("session", id),
            row("", self.dim(&path)),
        ];
        let msg = lines.join("\n");
        self.out(&msg);
    }

    pub(super) fn cmd_copy(&mut self) {
        if self.turn.last_reply.trim().is_empty() {
            return self.say("No assistant reply to copy yet.");
        }
        match desktop::copy_text(&self.turn.last_reply) {
            Ok(tool) => self.say(&format!("Copied the last reply ({tool}).")),
            Err(e) => self.error(&e),
        }
    }
}
