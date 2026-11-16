//! `/status` and `/stats`.

use crate::desktop;
use crate::tui::{Tui, fmt_tokens};
use rig_core::provider::Effort;
use rig_core::sync::MutexExt;

impl Tui {
    pub(super) fn cmd_stats(&mut self) {
        let id = self.session.locked().id.clone();
        let ctx = self.info.ctx_pct.map_or("unknown".into(), |p| {
            format!("{p}% of {}", fmt_tokens(self.info.context_window))
        });
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let session = usage_summary(&self.stats);
        self.say(&format!(
            "Session {id}\n{session}\ncontext {ctx}\n\nCalculating the last 30 days…"
        ));
        let root = self.app.paths.root.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let rolling = rig_core::session::Session::usage_all(
                &root,
                now.saturating_sub(30 * 24 * 60 * 60),
                now,
            );
            let rolling = match rolling {
                Ok(entries) => usage_summary(&crate::tui::Stats::from_entries(&entries)),
                Err(e) => format!("unavailable: {e}"),
            };
            let _ = tx.send(crate::runtime::Ev::Notice(format!(
                "Last 30 days · this profile, all projects\n{rolling}\n\nUsage is recorded from this version onward; estimates use list prices and exclude unpriced and subscription usage."
            )));
        });
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

fn usage_summary(s: &crate::tui::Stats) -> String {
    let t = s.total;
    format!(
        "{} jobs · {} in · {} out · {} cache read · {} cache write\nestimated cost ≈ ${:.4} · {} unpriced · {} subscription",
        s.turns,
        fmt_tokens(t.input),
        fmt_tokens(t.output),
        fmt_tokens(t.cached),
        fmt_tokens(t.cache_write),
        s.cost,
        s.unpriced_turns,
        s.plan_turns,
    )
}
