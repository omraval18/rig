//! The agent's questions: a list of options with a typed-answer row, in place of the composer.

use super::Tui;
use crate::question::Question;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc::Sender;

/// A question on screen, and where its answer goes.
pub struct PendingQuestion {
    pub view: Question,
    pub reply: Sender<Option<String>>,
}

impl Tui {
    pub(super) fn open_question(
        &mut self,
        text: String,
        options: Vec<String>,
        reply: Sender<Option<String>>,
    ) {
        self.prompt.completion = None;
        self.prompt.question = Some(PendingQuestion {
            view: Question::new(text, options),
            reply,
        });
        self.dirty = true;
    }

    pub(super) fn on_key_question(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let Some(pending) = self.prompt.question.as_mut() else {
            return;
        };
        let q = &mut pending.view;
        if let Some(text) = q.typing.as_mut() {
            match k.code {
                KeyCode::Esc if q.options.is_empty() => self.answer_question(None),
                KeyCode::Esc => q.typing = None,
                KeyCode::Enter => {
                    let answer = text.trim().to_string();
                    if !answer.is_empty() {
                        self.answer_question(Some(answer));
                    }
                }
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char('u') if ctrl => text.clear(),
                KeyCode::Char(c) if !ctrl && !alt => text.push(c),
                _ => {}
            }
            return;
        }
        match k.code {
            KeyCode::Esc => self.answer_question(None),
            KeyCode::Up => q.nav(-1),
            KeyCode::Down | KeyCode::Tab => q.nav(1),
            KeyCode::Char(d @ '1'..='9') => {
                let i = d as usize - '1' as usize;
                if i < q.options.len() {
                    let answer = q.options[i].clone();
                    self.answer_question(Some(answer));
                } else if i == q.own_row() {
                    q.selected = i;
                    q.typing = Some(String::new());
                }
            }
            KeyCode::Enter => {
                if q.selected == q.own_row() {
                    q.typing = Some(String::new());
                } else {
                    let answer = q.options[q.selected].clone();
                    self.answer_question(Some(answer));
                }
            }
            _ => {}
        }
    }

    /// Send the answer back to the agent, and leave a line in the transcript saying what was asked
    /// and answered.
    fn answer_question(&mut self, answer: Option<String>) {
        let Some(pending) = self.prompt.question.take() else {
            return;
        };
        let _ = pending.reply.send(answer.clone());
        match answer {
            Some(a) => self.say(&format!("↳ {} → {a}", pending.view.text)),
            None => self.say(&format!("↳ {} → (no answer)", pending.view.text)),
        }
        self.dirty = true;
    }
}
