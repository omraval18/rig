//! `/`, `@` and `$` completion in the prompt.

use super::Tui;
use crate::complete::{self, Kind, Sources, Trigger};

impl Tui {
    pub fn refresh_completion(&mut self) {
        if self.prompt.picker.is_some() || self.prompt.approval.is_some() {
            self.prompt.completion = None;
            return;
        }
        let text = self.prompt.composer.text();
        if self.prompt.suppress_completion.as_deref() == Some(text.as_str()) {
            return;
        }
        self.prompt.suppress_completion = None;
        let Some(trigger) =
            complete::detect(self.prompt.composer.chars(), self.prompt.composer.cursor)
        else {
            self.prompt.completion = None;
            return;
        };
        let keep_selection = self
            .prompt
            .completion
            .as_ref()
            .filter(|c| c.trigger == trigger)
            .map(|c| c.selected);
        let efforts = self.efforts_for_model();
        let src = Sources {
            index: &self.index,
            skills: &self.skills,
            models: &self.models,
            efforts: &efforts,
            current_model: &self.info.model,
        };
        let mut c = complete::build(trigger, self.prompt.composer.cursor, &src);
        if let Some(sel) = keep_selection {
            c.selected = sel.min(c.items.len().saturating_sub(1));
        }
        self.prompt.completion = Some(c);
        self.dirty = true;
    }

    pub(super) fn accept_completion(&mut self, enter: bool) {
        let Some(c) = self.prompt.completion.take() else {
            return;
        };
        let Some(item) = c.items.get(c.selected).cloned() else {
            self.prompt.completion = Some(c);
            return;
        };
        let typed = self.prompt.composer.text();
        // A mention typed out in full (`@calc.py`, `$deploy`) is already complete: Enter sends the message.
        let chars = self.prompt.composer.chars();
        let token: String = chars
            .get(c.range.0..c.range.1.min(chars.len()))
            .map(|r| r.iter().collect())
            .unwrap_or_default();
        if enter
            && matches!(c.trigger, Trigger::At { .. } | Trigger::Dollar { .. })
            && item.kind != Kind::Dir
            && token == item.insert
        {
            self.submit();
            return;
        }
        match &c.trigger {
            Trigger::Slash { .. } => {
                // `/model` typed in full and Enter: run it (opens the picker) instead of expanding.
                if enter && typed.trim() == item.label {
                    self.submit();
                    return;
                }
                self.prompt
                    .composer
                    .replace_range(0, self.prompt.composer.cursor, &item.insert);
                if enter && item.complete_command {
                    self.submit();
                    return;
                }
            }
            Trigger::Arg { query, .. } => {
                if enter && query.is_empty() {
                    self.submit();
                    return;
                }
                // `/skills list` typed in full and Enter: run it rather than just re-inserting `list `.
                let exact = item.insert.trim() == query.trim();
                self.prompt
                    .composer
                    .replace_range(c.range.0, c.range.1, &item.insert);
                if enter && (exact || (item.kind != Kind::Skill && !item.insert.ends_with(' '))) {
                    self.submit();
                    return;
                }
            }
            Trigger::At { .. } => {
                // Enter mentions a folder; Tab opens it so you can keep browsing.
                let open_folder = item.kind == Kind::Dir && !enter;
                let tail = if open_folder { "" } else { " " };
                let insert = if open_folder {
                    item.insert.trim_end_matches('"').to_string()
                } else {
                    item.insert.clone()
                };
                self.prompt.composer.replace_range(
                    c.range.0,
                    c.range.1,
                    &format!("{insert}{tail}"),
                );
                if open_folder && item.insert.ends_with("/\"") {
                    // Re-quote after the slash so the cursor stays inside the quotes.
                    self.prompt.composer.insert_str("\"");
                    self.prompt.composer.left();
                }
            }
            Trigger::Dollar { .. } => {
                self.prompt.composer.replace_range(
                    c.range.0,
                    c.range.1,
                    &format!("{} ", item.insert),
                );
            }
        }
        self.prompt.suppress_completion = None;
        self.refresh_completion();
        self.dirty = true;
    }
}
