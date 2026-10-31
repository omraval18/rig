//! `/loop` (also `/goal`): keep working toward one objective across turns. A turn that ends while
//! the goal is open starts another one, until the model reports the goal complete or blocked, the
//! round limit is reached, or the person interrupts.

use super::Tui;
use rig_core::message::Message;
use rig_core::sync::MutexExt;

/// Turns a `/loop` may run before it stops and asks the person to decide.
pub const MAX_ROUNDS: u32 = 10;

pub struct Goal {
    pub objective: String,
    /// Turns started so far, counting the first.
    pub rounds: u32,
}

impl Tui {
    /// `/loop <objective>` starts a goal; `/loop` shows it; `/loop stop` drops it.
    pub(crate) fn cmd_loop(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() {
            return match &self.goal {
                Some(g) => self.say(&format!(
                    "Goal: {}\nRound {} of {}. Esc or Ctrl-C stops it; `/loop stop` drops it.",
                    g.objective, g.rounds, MAX_ROUNDS
                )),
                None => self.say("No goal. Start one with /loop <objective>."),
            };
        }
        if arg == "stop" {
            self.goal = None;
            return self.say("Goal dropped.");
        }
        if !self.idle_agent() {
            return;
        }
        self.goal = Some(Goal {
            objective: arg.to_string(),
            rounds: 1,
        });
        self.launch_goal_round(&format!(
            "Goal: {arg}\n\nWork toward this across as many steps as it takes. When the whole goal is done and checked, call update_goal with status complete. If you cannot continue without the person, ask them with ask_user first; if they cannot help, call update_goal with status blocked and say why. Otherwise keep working without calling update_goal."
        ));
    }

    /// Called when a turn ends normally. Continues the goal, or settles it.
    pub(super) fn goal_after_turn(&mut self) {
        let signal = self.shared.goal_signal.locked().take();
        let Some(goal) = self.goal.as_mut() else {
            return;
        };
        match signal.as_deref() {
            Some("complete") => {
                let text = format!("Goal complete after {} round(s).", goal.rounds);
                self.goal = None;
                self.say(&text);
            }
            Some("blocked") => {
                let text = format!(
                    "Goal blocked after {} round(s). Tell me what you need, then /loop again.",
                    goal.rounds
                );
                self.goal = None;
                self.say(&text);
            }
            _ if goal.rounds >= MAX_ROUNDS => {
                let text = format!(
                    "Goal stopped after {MAX_ROUNDS} rounds without a result. Check the progress, then /loop again to continue."
                );
                self.goal = None;
                self.say(&text);
            }
            _ => {
                goal.rounds += 1;
                let objective = goal.objective.clone();
                let round = goal.rounds;
                self.say(&format!("↻ goal round {round} of {MAX_ROUNDS}"));
                self.launch_goal_round(&format!(
                    "Continue toward the goal: {objective}\n\nCheck what is still left, then keep going. Call update_goal with status complete when it is done and checked, or blocked if you need the person and have asked them."
                ));
            }
        }
    }

    fn launch_goal_round(&mut self, text: &str) {
        self.launch(crate::runtime::Job::Turn(Message::user(text)));
    }
}
