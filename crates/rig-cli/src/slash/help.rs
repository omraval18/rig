//! `/help`: every command and shortcut in one searchable list.

use crate::picker::{PickItem, PickKind};
use crate::tui::Tui;
use rig_core::commands::{COMMANDS, Category};

const SHORTCUTS: &[(&str, &str)] = &[
    ("Ctrl+P", "choose a model"),
    ("Ctrl+O", "switch or create a profile"),
    ("Shift+Tab", "cycle permissions"),
    ("Enter while working", "queue a message"),
    ("Esc", "interrupt the turn, or close a list"),
    ("Alt+Enter, Ctrl+J", "new line"),
    ("Ctrl+V", "paste an image"),
    ("PageUp, PageDown, End", "scroll, jump to the latest"),
    ("@", "mention a file or folder"),
    ("$", "use a skill"),
];

impl Tui {
    pub(super) fn open_help(&mut self) {
        let commands = Category::ALL.into_iter().flat_map(|cat| {
            COMMANDS
                .iter()
                .filter(move |s| s.category == cat)
                .map(move |s| {
                    PickItem::new(s.usage, s.name)
                        .detail(s.summary)
                        .group(cat.title())
                })
        });
        let keys = SHORTCUTS
            .iter()
            .map(|(k, what)| PickItem::new(*k, "").detail(*what).group("Shortcuts"));
        let items = commands.chain(keys).collect();
        self.open_picker(PickKind::Help, "Commands and shortcuts", items);
    }

    /// Run the chosen command, or put it in the prompt when it needs an argument.
    pub(super) fn help_picked(&mut self, name: &str) {
        let Some(spec) = rig_core::commands::lookup(name) else {
            return;
        };
        if spec.has_args && !crate::complete::runs_bare(spec.cmd) {
            self.prompt.composer.insert_str(&format!("/{name} "));
            self.refresh_completion();
        } else {
            self.run_slash(&format!("/{name}"));
        }
    }
}
