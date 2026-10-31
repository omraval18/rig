//! Keyboard handling for the prompt, the approval panel and pickers.

use super::Tui;
use crate::composer::Composer;
use crate::picker::{PickItem, PickKind, Picker};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rig_core::permission::Approval;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

impl Tui {
    pub(super) fn on_key(&mut self, k: KeyEvent) {
        self.dirty = true;
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);

        if ctrl && k.code == KeyCode::Char('c') {
            return self.on_ctrl_c();
        }
        if self.prompt.approval.is_some() {
            return self.on_key_approval(k);
        }
        if self.prompt.question.is_some() {
            return self.on_key_question(k);
        }
        if self.prompt.entry.is_some() {
            return self.on_key_entry(k);
        }
        if self.prompt.picker.is_some() {
            return self.on_key_picker(k);
        }
        let page = self.display.height.saturating_sub(5).max(1) as isize;
        let view_rows = self.display.height.saturating_sub(4);
        match k.code {
            KeyCode::Char('d') if ctrl => {
                if self.prompt.composer.is_empty() {
                    self.quit = true;
                } else {
                    self.prompt.composer.delete();
                }
            }
            KeyCode::Char('l') if ctrl => {
                let _ = self.display.screen.invalidate();
            }
            KeyCode::PageUp => self.display.transcript.scroll_by(page, view_rows),
            KeyCode::PageDown => self.display.transcript.scroll_by(-page, view_rows),
            KeyCode::End if self.prompt.composer.is_empty() => self.display.transcript.scroll = 0,
            KeyCode::Char('p') if ctrl => self.open_model_picker(),
            KeyCode::Char('v') if ctrl => self.paste_clipboard_image(),
            KeyCode::BackTab => self.cycle_permissions(),
            KeyCode::Esc => self.on_escape(),
            KeyCode::Enter if alt || shift || ctrl => self.insert_newline(),
            KeyCode::Char('j') if ctrl => self.insert_newline(),
            KeyCode::Enter if self.prompt.composer.continues_line() => {
                self.prompt.composer.backspace();
                self.insert_newline();
            }
            KeyCode::Enter if self.has_completion_choice() => self.accept_completion(true),
            KeyCode::Enter => self.submit(),
            KeyCode::Tab if self.has_completion_choice() => self.accept_completion(false),
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Char('n') if ctrl => {
                if let Some(c) = self.prompt.completion.as_mut() {
                    c.nav(1);
                }
            }
            code => {
                if let Some(command) = edit_command(code, k.modifiers) {
                    self.edit(command);
                } else if let KeyCode::Char(ch) = code
                    && !ctrl
                    && !alt
                {
                    self.edit(|c| c.insert_char(ch));
                }
            }
        }
    }

    fn has_completion_choice(&self) -> bool {
        self.prompt
            .completion
            .as_ref()
            .is_some_and(|c| c.selected_item().is_some())
    }

    fn insert_newline(&mut self) {
        self.prompt.composer.newline();
        self.refresh_completion();
    }

    /// Esc closes completion first, then interrupts a running turn.
    fn on_escape(&mut self) {
        if self.prompt.completion.is_some() {
            self.prompt.suppress_completion = Some(self.prompt.composer.text());
            self.prompt.completion = None;
        } else if self.turn.running.is_some() || self.turn.compacting {
            self.cancel_turn();
        }
    }

    fn cancel_turn(&mut self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.notice("interrupting…");
    }

    /// Up moves through completion items, then lines, then older prompts.
    fn move_up(&mut self) {
        if let Some(c) = self
            .prompt
            .completion
            .as_mut()
            .filter(|c| !c.items.is_empty())
        {
            c.nav(-1);
        } else if !self.prompt.composer.up() {
            self.prompt.composer.history_prev();
            self.refresh_completion();
        }
    }

    fn move_down(&mut self) {
        if let Some(c) = self
            .prompt
            .completion
            .as_mut()
            .filter(|c| !c.items.is_empty())
        {
            c.nav(1);
        } else if !self.prompt.composer.down() {
            self.prompt.composer.history_next();
            self.refresh_completion();
        }
    }

    pub(super) fn edit(&mut self, f: impl FnOnce(&mut Composer)) {
        f(&mut self.prompt.composer);
        self.refresh_completion();
    }

    pub(super) fn on_ctrl_c(&mut self) {
        // Ctrl-C first backs out of a text box, so it never closes rig by accident.
        if self.prompt.entry.take().is_some() {
            return;
        }
        // Ctrl-C also declines a question, so the agent carries on without an answer.
        if let Some(pending) = self.prompt.question.take() {
            let _ = pending.reply.send(None);
            return;
        }
        if self.prompt.approval.is_some() || self.turn.running.is_some() || self.turn.compacting {
            if let Some(a) = self.prompt.approval.take() {
                let _ = a.reply.send(Approval::No);
            }
            self.cancel_turn();
        } else if self.prompt.picker.is_some() {
            self.prompt.picker = None;
        } else if !self.prompt.composer.is_empty() {
            self.prompt.composer.clear();
            self.prompt.completion = None;
        } else if self
            .ctrl_c_at
            .is_some_and(|t| t.elapsed() < Duration::from_secs(2))
        {
            self.quit = true;
        } else {
            self.ctrl_c_at = Some(Instant::now());
            self.notice("press Ctrl-C again to exit");
        }
    }

    pub(super) fn on_key_approval(&mut self, k: KeyEvent) {
        let Some(a) = self.prompt.approval.as_mut() else {
            return;
        };
        let choice = match k.code {
            KeyCode::Char('y') => Some(Approval::Once),
            KeyCode::Char('a') => Some(Approval::Always),
            KeyCode::Char('n') | KeyCode::Esc => Some(Approval::No),
            KeyCode::Left => {
                a.view.selected = (a.view.selected + 2) % 3;
                None
            }
            KeyCode::Right | KeyCode::Tab => {
                a.view.selected = (a.view.selected + 1) % 3;
                None
            }
            KeyCode::Enter => {
                Some([Approval::Once, Approval::Always, Approval::No][a.view.selected])
            }
            _ => None,
        };
        if let Some(mut choice) = choice
            && let Some(a) = self.prompt.approval.take()
        {
            if choice == Approval::Always && a.remember.is_none() {
                choice = Approval::Once;
            }
            let _ = a.reply.send(choice);
        }
    }

    pub(super) fn on_key_picker(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let Some(p) = self.prompt.picker.as_mut() else {
            return;
        };
        match k.code {
            KeyCode::Esc => self.prompt.picker = None,
            KeyCode::Up => p.nav(-1),
            KeyCode::Down | KeyCode::Tab => p.nav(1),
            KeyCode::Char('p') if ctrl => p.nav(-1),
            KeyCode::Char('n') if ctrl => p.nav(1),
            KeyCode::Backspace => p.backspace(),
            KeyCode::Enter => {
                let picked = p.selected_item().cloned().map(|i| (p.kind, i));
                self.prompt.picker = None;
                if let Some((kind, item)) = picked {
                    self.on_pick(kind, item);
                }
            }
            KeyCode::Char(c) if !ctrl => p.type_char(c),
            _ => {}
        }
    }

    pub fn open_picker(&mut self, kind: PickKind, title: &str, items: Vec<PickItem>) {
        if items.is_empty() {
            self.say("Nothing to choose from.");
            return;
        }
        self.prompt.completion = None;
        self.prompt.picker = Some(Picker::new(kind, title, items));
        self.dirty = true;
    }
}

/// Cursor and text-editing keys. Each is a single `Composer` operation.
fn edit_command(code: KeyCode, mods: KeyModifiers) -> Option<fn(&mut Composer)> {
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let alt = mods.contains(KeyModifiers::ALT);
    Some(match code {
        KeyCode::Left if ctrl || alt => Composer::word_left,
        KeyCode::Right if ctrl || alt => Composer::word_right,
        KeyCode::Char('b') if alt => Composer::word_left,
        KeyCode::Char('f') if alt => Composer::word_right,
        KeyCode::Left => Composer::left,
        KeyCode::Char('b') if ctrl => Composer::left,
        KeyCode::Right => Composer::right,
        KeyCode::Char('f') if ctrl => Composer::right,
        KeyCode::Home => Composer::home,
        KeyCode::Char('a') if ctrl => Composer::home,
        KeyCode::End => Composer::end,
        KeyCode::Char('e') if ctrl => Composer::end,
        KeyCode::Char('k') if ctrl => Composer::kill_to_end,
        KeyCode::Char('u') if ctrl => Composer::kill_to_start,
        KeyCode::Char('w') if ctrl => Composer::kill_word_back,
        KeyCode::Char('y') if ctrl => Composer::yank,
        KeyCode::Backspace if alt => Composer::kill_word_back,
        KeyCode::Backspace => Composer::backspace,
        KeyCode::Delete => Composer::delete,
        _ => return None,
    })
}
