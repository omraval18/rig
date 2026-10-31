//! The interactive terminal app: event loop, keys, completion, and the turn lifecycle.
//!
//! One thread owns the terminal. The agent runs on a worker (`runtime`), so the prompt
//! stays live during a turn: Enter steers, Esc interrupts, approvals appear in place.

use crate::app::{App, RunOpts, Started};
use crate::complete::Completion;
use crate::composer::Composer;
use crate::markdown::Styler;
use crate::picker::Picker;
use crate::runtime::Ev;
use crate::screen::Screen;
use crate::transcript::Transcript;
use crate::view::ApprovalView;
use crossterm::event::{self};
use crossterm::terminal;
use rig_core::agent::Agent;
use rig_core::message::{Message, Role};
use rig_core::permission::{Approval, Policy};
use rig_core::provider::{Effort, ModelInfo, Provider, Usage};
use rig_core::session::Session;
use rig_core::skills::Skill;
use rig_core::sync::MutexExt;
use rig_core::tool::Shared;
use rig_provider::ProviderSpec;
use rig_tools::index::Index;
use std::collections::VecDeque;
use std::io::{IsTerminal, Stdout, stdout};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

mod agent_events;
mod completion;
mod draw;
mod entry;
mod events;
mod format;
mod keys;
mod output;
mod provider;
mod recap;
mod state;
mod submit;
#[cfg(test)]
mod tests;
mod turn;

pub use format::{fmt_tokens, is_hidden, visible_text};
pub(crate) use state::{Display, Info, PendingApproval, PendingChange, Prompt, Stats, Turn};
use state::{Part, RunTool};

pub struct Tui {
    pub app: App,
    pub prompt: Prompt,
    pub turn: Turn,
    pub display: Display,
    pub agent: Option<Box<Agent>>,
    pub tx: Sender<Ev>,
    rx: Receiver<Ev>,
    pub steer: Arc<Mutex<VecDeque<Message>>>,
    pub session: Arc<Mutex<Session>>,
    pub shared: Arc<Shared>,
    pub policy: Policy,
    pub index: Index,
    pub skills: Vec<Skill>,
    pub models: Vec<ModelInfo>,
    pub info: Info,
    pub pending: PendingChange,
    pub show_reasoning: bool,
    pub stats: Stats,
    pub quit: bool,
    /// Set by `/profile`: on quit, rig starts again in this profile.
    pub switch_profile: Option<String>,
    /// When the footer last looked for a staged update (see `main_loop`).
    pub update_poll: Instant,
    ctrl_c_at: Option<Instant>,
    pub dirty: bool,
    saved_allowlist: Vec<String>,
}

/// Puts the terminal back the way we found it, even on early return.
struct RawGuard;

impl RawGuard {
    /// Take over the whole window: the alternate screen (your shell is back untouched on exit),
    /// raw keys, bracketed paste, and the mouse wheel for scrolling.
    fn enter(mouse: bool) -> std::io::Result<Self> {
        terminal::enable_raw_mode()?;
        crossterm::execute!(
            stdout(),
            terminal::EnterAlternateScreen,
            event::EnableBracketedPaste
        )?;
        if mouse {
            crossterm::execute!(stdout(), event::EnableMouseCapture)?;
        }
        Ok(RawGuard)
    }
}

fn restore_terminal() {
    let _ = crossterm::execute!(
        stdout(),
        event::DisableMouseCapture,
        event::DisableBracketedPaste,
        crossterm::cursor::Show,
        terminal::LeaveAlternateScreen
    );
    let _ = terminal::disable_raw_mode();
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

pub fn run(app: App, opts: RunOpts) -> i32 {
    if !std::io::stdin().is_terminal() || !stdout().is_terminal() {
        eprintln!("error: rig needs an interactive terminal. For scripts use: rig ask \"...\"");
        return 1;
    }
    let started = match app.start(&opts, true) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let resumed = opts.cont || opts.resume.is_some();
    let first_message = opts.first_message;
    let guard = match RawGuard::enter(app.state.mouse.unwrap_or(true)) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("error: cannot use the terminal: {e}");
            return 1;
        }
    };
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));
    let mut tui = Tui::new(app, started);
    tui.banner(resumed);
    tui.fetch_models();
    if let Some(first) = first_message {
        tui.prompt.composer.insert_str(&first);
        tui.submit();
    }
    tui.main_loop();
    let farewell = tui.farewell();
    let switch = tui.switch_profile.take();
    drop(guard);
    if let Some(f) = farewell {
        println!("{f}");
    }
    match switch {
        Some(name) => restart_in_profile(&name),
        None => 0,
    }
}

/// Replace this process with a fresh `rig` in another profile. Every path, login, key and setting
/// is per profile, so starting over is simpler and safer than swapping them all in place.
fn restart_in_profile(name: &str) -> i32 {
    use std::os::unix::process::CommandExt;
    println!("Switching to profile '{name}'…");
    let exe = std::env::current_exe().unwrap_or_else(|_| "rig".into());
    let err = std::process::Command::new(exe)
        .arg("--profile")
        .arg(name)
        .exec();
    eprintln!("error: cannot restart rig: {err}. Run: rig --profile {name}");
    1
}

impl Tui {
    /// Wipe the window down to a fresh header: the start of a new session, or one just resumed.
    pub fn reset_screen(&mut self) {
        self.display.transcript.clear();
        self.display.last_part = Part::Start;
        self.banner(false);
        self.dirty = true;
    }

    /// One line on exit saying how to pick this conversation back up. A session nobody typed in is deleted.
    pub(super) fn farewell(&self) -> Option<String> {
        let talked =
            self.agent.as_ref()?.history.iter().any(|m| {
                m.role == Role::User && !is_hidden(m) && !rig_core::compact::is_checkpoint(m)
            });
        if !talked {
            // Opened and closed without a word: do not leave an empty session behind.
            let path = self.session.locked().path.clone();
            let _ = std::fs::remove_file(rig_core::session::archive_path(&path));
            let _ = std::fs::remove_file(path);
            return None;
        }
        let resume = if self.app.paths.is_default_profile() && self.switch_profile.is_none() {
            "rig -c".to_string()
        } else {
            format!("rig --profile {} -c", self.app.paths.profile)
        };
        Some(self.dim(&format!("Session saved. Continue it here with: {resume}")))
    }

    pub(super) fn new(app: App, s: Started) -> Tui {
        let (tx, rx) = channel();
        let (width, height) = terminal::size().map_or((80, 24), |(w, h)| (w as usize, h as usize));
        let color = std::env::var_os("NO_COLOR").is_none();
        let agent = s.agent;
        let info = Info {
            spec: s.spec,
            model: s.model,
            effort: agent.effort,
            context_window: agent.context_window,
            ctx_pct: agent.context_percent(),
        };
        let app_show_reasoning = app.state.show_reasoning.unwrap_or(false);
        let history = app.load_history();
        let index = Index::spawn(app.cwd.clone(), app.paths.home.clone());
        let shared = agent.ctx.shared.clone();
        let policy = agent.policy.clone();
        let saved_allowlist = policy.allowlist();
        let mut t = Tui {
            app,
            prompt: Prompt::new(history),
            turn: Turn::default(),
            display: Display::new(width, height, color),
            agent: Some(Box::new(agent)),
            tx,
            rx,
            steer: Arc::default(),
            session: Arc::new(Mutex::new(s.session)),
            shared,
            policy,
            index,
            skills: s.skills,
            models: Vec::new(),
            info,
            pending: PendingChange::default(),
            show_reasoning: app_show_reasoning,
            stats: Stats::default(),
            quit: false,
            switch_profile: None,
            update_poll: Instant::now(),
            ctrl_c_at: None,
            dirty: true,
            saved_allowlist,
        };
        t.dirty = true;
        t
    }
}
