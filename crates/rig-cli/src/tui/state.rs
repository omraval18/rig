//! The pieces of state `Tui` is made of, grouped by what changes them.

use super::*;

pub struct Info {
    pub spec: ProviderSpec,
    pub model: String,
    pub effort: Option<Effort>,
    pub context_window: u64,
    pub ctx_pct: Option<u8>,
}

#[derive(Default)]
pub struct PendingChange {
    pub provider: Option<Box<dyn Provider>>,
    pub effort: Option<Option<Effort>>,
    pub context_window: Option<u64>,
    pub auto_compact_percent: Option<u8>,
}

impl PendingChange {
    pub fn apply(self, agent: &mut Agent) {
        if let Some(p) = self.provider {
            agent.provider = p;
        }
        if let Some(e) = self.effort {
            agent.effort = e;
        }
        if let Some(w) = self.context_window {
            agent.context_window = w;
        }
        if let Some(p) = self.auto_compact_percent {
            agent.auto_compact_percent = p;
        }
    }

    /// Later changes win over earlier ones.
    pub(super) fn merge(&mut self, later: PendingChange) {
        self.provider = later.provider.or(self.provider.take());
        self.effort = later.effort.or(self.effort);
        self.context_window = later.context_window.or(self.context_window);
        self.auto_compact_percent = later.auto_compact_percent.or(self.auto_compact_percent);
    }
}

pub struct PendingApproval {
    pub view: ApprovalView,
    pub reply: Sender<Approval>,
    pub remember: Option<String>,
}

#[derive(Default)]
pub struct Stats {
    pub turns: u64,
    pub total: Usage,
    /// Estimated cost in USD, from list prices, of the turns we had a price for.
    pub cost: f64,
    /// Turns on a model with no list price here, so they are not in `cost`.
    pub unpriced_turns: u64,
    /// Turns on a ChatGPT plan: included in the subscription, not billed per token.
    pub plan_turns: u64,
}

/// What the last printed transcript block was. A blank line separates different kinds, never
/// lines of the same kind, so replies, tool activity, notes and your own messages each read as a unit.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// Nothing printed yet (or the screen was cleared): no separator needed.
    Start,
    Banner,
    User,
    Reply,
    Think,
    Tool,
    Note,
}

pub struct RunTool {
    pub name: String,
    pub desc: String,
    pub at: Instant,
}

/// What is typed, and what is open over it (completion, a picker, an approval).
pub struct Prompt {
    pub composer: Composer,
    pub completion: Option<Completion>,
    /// Text for which the user dismissed completion, so it does not pop back up.
    pub suppress_completion: Option<String>,
    pub picker: Option<Picker>,
    /// The text box for an API key or a profile name, over the composer.
    pub entry: Option<crate::entry::Entry>,
    /// A question from the agent, over the composer.
    pub question: Option<super::question::PendingQuestion>,
    pub approval: Option<PendingApproval>,
    pub attachments: Vec<crate::attach::Attachment>,
    pub next_image: usize,
}

impl Prompt {
    pub fn new(history: Vec<String>) -> Self {
        Self {
            composer: Composer::new(history),
            completion: None,
            suppress_completion: None,
            picker: None,
            entry: None,
            question: None,
            approval: None,
            attachments: Vec::new(),
            next_image: 1,
        }
    }
}

/// The turn in flight, or the one that just ended.
#[derive(Default)]
pub struct Turn {
    pub running: Option<Instant>,
    pub compacting: bool,
    /// The unfinished line of streamed text, and of reasoning.
    pub partial: String,
    pub reasoning_buf: String,
    pub usage: Usage,
    /// The assistant text of this turn so far, and of the last finished one (`/copy`).
    pub reply: String,
    pub last_reply: String,
    /// The prompt of the turn in flight, so a failed turn can hand it back for a retry.
    pub last_prompt: String,
    pub rolled_back: bool,
    /// Tool calls in flight, oldest first. Their rows live in the redrawn area (they pulse) until they end.
    pub tools: VecDeque<RunTool>,
}

/// How the conversation is drawn: the window, the scrollback, and transient decorations.
pub struct Display {
    pub screen: Screen<std::io::BufWriter<Stdout>>,
    pub color: bool,
    pub width: usize,
    pub height: usize,
    /// The conversation as shown: wrapped to the window, scrollable, cleared by /new.
    pub transcript: Transcript,
    pub styler: Styler,
    pub last_part: Part,
    pub notice: Option<(String, Instant)>,
    pub spinner: usize,
    pub last_tick: Instant,
}

impl Display {
    pub fn new(width: usize, height: usize, color: bool) -> Self {
        Self {
            screen: Screen::new(std::io::BufWriter::with_capacity(1 << 16, stdout())),
            color,
            width,
            height,
            transcript: Transcript::new(width, color),
            styler: Styler::new(color),
            last_part: Part::Banner,
            notice: None,
            spinner: 0,
            last_tick: Instant::now(),
        }
    }
}
