//! Slash-command behavior and the pickers they open. One file per group of commands.

mod account;
mod info;
mod model;
mod session;
mod settings;
mod skills;

use crate::picker::{PickItem, PickKind};
use crate::tui::Tui;
use rig_core::commands::{self, Cmd};
use std::path::PathBuf;

const BUSY: &str = "Wait for the current turn to finish first (Esc interrupts it).";

/// The first non-empty line of `s`, trimmed and cut to `max` characters.
fn first_line(s: &str, max: usize) -> String {
    let l = s
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    if l.chars().count() > max {
        format!("{}…", l.chars().take(max).collect::<String>())
    } else {
        l.to_string()
    }
}

impl Tui {
    pub fn run_slash(&mut self, line: &str) {
        let body = line.trim_start_matches('/');
        let (name, arg) = body
            .split_once(char::is_whitespace)
            .map_or((body, ""), |(n, a)| (n, a.trim()));
        let Some(spec) = commands::lookup(name) else {
            self.error(&format!(
                "unknown command '/{name}'. Type / to see the list, or /help."
            ));
            return;
        };
        match spec.cmd {
            Cmd::Help => {
                let t = commands::help_text();
                self.out(t.trim_end());
            }
            Cmd::Quit => self.quit = true,
            Cmd::Version => self.say(&format!("rig {}", env!("CARGO_PKG_VERSION"))),
            Cmd::New => self.cmd_new(),
            Cmd::Resume => self.cmd_resume(),
            Cmd::Rename => self.cmd_rename(arg),
            Cmd::Fork => self.cmd_fork(),
            Cmd::Compact => self.cmd_compact(),
            Cmd::Summary => self.cmd_summary(),
            Cmd::Loop => self.cmd_loop(arg),
            Cmd::Mcp => self.cmd_mcp(),
            Cmd::Undo => self.cmd_undo(),
            Cmd::Paste => self.paste_clipboard_image(),
            Cmd::Copy => self.cmd_copy(),
            Cmd::Model => {
                if arg.is_empty() {
                    self.open_model_picker();
                } else {
                    self.cmd_model(arg);
                }
            }
            Cmd::Effort => self.cmd_effort(arg),
            Cmd::Reasoning => self.cmd_reasoning(),
            Cmd::Permissions => self.cmd_permissions(arg),
            Cmd::Allowlist => self.cmd_allowlist(arg),
            Cmd::Provider => self.cmd_provider(),
            Cmd::Profile => self.cmd_profile(arg),
            Cmd::Login => self.cmd_login(),
            Cmd::Logout => self.cmd_logout(),
            Cmd::Stats => self.cmd_stats(),
            Cmd::Status => self.cmd_status(),
            Cmd::Settings => self.cmd_settings(arg),
            Cmd::Skills => self.cmd_skills(arg),
        }
    }

    pub fn on_pick(&mut self, kind: PickKind, item: PickItem) {
        match kind {
            PickKind::Model => self.cmd_model(&item.value),
            PickKind::Effort => self.cmd_effort(&item.value),
            PickKind::Permissions => self.cmd_permissions(&item.value),
            PickKind::Resume => self.load_session(PathBuf::from(&item.value)),
            PickKind::Fork => self.fork_at(item.value.parse().unwrap_or(0)),
            PickKind::Provider => self.pick_provider(&item.value),
            PickKind::Profile => self.cmd_profile(&item.value),
            PickKind::Settings => self.pick_setting(&item.value),
            PickKind::Skills => {
                self.prompt
                    .composer
                    .insert_str(&format!("${} ", item.value));
                self.refresh_completion();
            }
        }
    }
}
