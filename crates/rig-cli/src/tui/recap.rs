//! The header shown at the top of a screen, and the replay of a resumed conversation.

use super::format::{is_hidden, visible_text};
use super::{Part, Tui};
use crate::markdown::Styler;
use crate::style;
use rig_core::message::{Block, Message, Role};

impl Tui {
    pub(super) fn banner(&mut self, resumed: bool) {
        let cwd = self.app.cwd.display().to_string();
        let home = self
            .app
            .paths
            .home
            .as_ref()
            .map(|h| h.display().to_string());
        let shown = match home {
            Some(h) if cwd.starts_with(&h) => format!("~{}", &cwd[h.len()..]),
            _ => cwd,
        };
        let title = if self.display.color {
            format!(
                "{}{}{}{} · {}/{} · {}",
                style::BOLD,
                format_args!("rig {}", env!("CARGO_PKG_VERSION")),
                style::RESET,
                style::DIM,
                self.info.spec.name,
                style::paint(true, style::CYAN, &self.info.model),
                format_args!("{shown}{}", style::RESET)
            )
        } else {
            format!(
                "rig {} · {}/{} · {shown}",
                env!("CARGO_PKG_VERSION"),
                self.info.spec.name,
                self.info.model
            )
        };
        let tips = self.dim("/ commands · @ files · $ skills · Ctrl+V paste image · Esc interrupts · Shift+Tab permissions · Ctrl+P model");
        self.display
            .transcript
            .push_text(&format!("{title}\n{tips}"));
        self.display.last_part = Part::Banner;
        if !self.signed_in() {
            let hint = crate::app::sign_in_hint(&self.info.spec);
            self.out(&hint);
        }
        if resumed {
            let history = self
                .agent
                .as_ref()
                .map(|a| a.history.clone())
                .unwrap_or_default();
            self.recap(&history);
        }
    }

    /// Replay the tail of a resumed conversation through the same renderers a live turn uses,
    /// so it looks identical: your messages as cards, styled replies, tool lines.
    pub fn recap(&mut self, history: &[Message]) {
        const PROMPTS: usize = 40;
        const MAX_REPLY_LINES: usize = 300;
        let shown = |m: &Message| !is_hidden(m) && !rig_core::compact::is_checkpoint(m);
        let prompt_at: Vec<usize> = history
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == Role::User && shown(m) && !visible_text(m).trim().is_empty())
            .map(|(i, _)| i)
            .collect();
        let from = prompt_at
            .len()
            .checked_sub(PROMPTS)
            .map_or(0, |k| prompt_at[k]);
        let earlier = prompt_at.iter().filter(|&&i| i < from).count();
        let head = if earlier > 0 {
            format!(
                "resumed session · {} messages · {earlier} earlier prompt(s) not shown",
                history.len()
            )
        } else {
            format!("resumed session · {} messages", history.len())
        };
        let t = self.dim(&head);
        self.out(&t);
        let mut styler = Styler::new(self.display.color);
        for m in history[from..].iter().filter(|m| shown(m)) {
            if m.role == Role::User {
                let text = visible_text(m);
                if !text.trim().is_empty() {
                    self.echo_user(&text);
                }
                continue;
            }
            for block in &m.blocks {
                match block {
                    Block::Text { text } => {
                        let lines: Vec<&str> = text.trim().lines().collect();
                        for line in lines.iter().take(MAX_REPLY_LINES) {
                            let styled = styler.line(line);
                            self.print_part(Part::Reply, &styled);
                        }
                        if lines.len() > MAX_REPLY_LINES {
                            let t = self
                                .dim(&format!("… {} more lines", lines.len() - MAX_REPLY_LINES));
                            self.print_part(Part::Reply, &t);
                        }
                    }
                    Block::ToolCall { id, name, args } => {
                        let desc = self
                            .agent
                            .as_ref()
                            .and_then(|a| a.tools.get(name).map(|t| t.describe(args)))
                            .unwrap_or_default();
                        let ok = history
                            .iter()
                            .flat_map(|m| &m.blocks)
                            .find_map(|b| match b {
                                Block::ToolResult {
                                    id: rid, is_error, ..
                                } if rid == id => Some(!*is_error),
                                _ => None,
                            });
                        let line = self.tool_row(Some(ok.unwrap_or(false)), name, &desc, None);
                        self.print_part(Part::Tool, &line);
                    }
                    _ => {}
                }
            }
        }
    }
}
