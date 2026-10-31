mod ansi;
mod app;
mod attach;
mod complete;
mod composer;
mod config;
mod desktop;
mod entry;
mod keys;
mod markdown;
mod picker;
mod plain;
mod profile;
mod question;
mod runtime;
mod screen;
mod slash;
mod state;
mod style;
mod subcommands;
mod transcript;
mod tui;
mod update;
mod view;

use app::{App, RunOpts};
use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rig", version, about = "A small, fast coding agent harness")]
struct Cli {
    #[command(flatten)]
    opts: Opts,
    /// Open the interactive UI and send this as the first message (use `--` before text that starts with a dash)
    #[arg(trailing_var_arg = true)]
    prompt: Vec<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Args)]
struct Opts {
    /// Model as provider/model, e.g. chatgpt/gpt-6-sol or anthropic/claude-sonnet-5-5
    #[arg(short, long, global = true)]
    model: Option<String>,
    /// Reasoning effort: low, medium, high, xhigh, none
    #[arg(short = 'r', long, global = true)]
    effort: Option<String>,
    /// Permissions: ask, read-only, low (auto), medium, high (full-access)
    #[arg(long, global = true, value_name = "LEVEL")]
    auto: Option<String>,
    /// Continue the latest session in this directory
    #[arg(short = 'c', long = "continue", global = true)]
    cont: bool,
    /// Resume a session by id (or unique prefix)
    #[arg(long, global = true, value_name = "ID")]
    resume: Option<String>,
    /// Use this profile for this run (its own logins, keys and settings); see `rig profile`
    #[arg(long, global = true, value_name = "NAME")]
    profile: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Send one prompt and print the answer (for scripts and CI)
    Ask {
        /// The prompt; reads stdin when omitted. @path and $skill work here too
        prompt: Vec<String>,
        /// Print a JSON result instead of streaming text
        #[arg(long)]
        json: bool,
    },
    /// Sign in with a ChatGPT subscription: rig login codex (into the active profile)
    Login {
        #[arg(default_value = "codex")]
        provider: String,
    },
    /// Remove the active profile's ChatGPT credentials
    Logout,
    /// Manage profiles: separate logins, API keys and settings
    #[command(subcommand)]
    Profile(ProfileCmd),
    /// Save API keys in the active profile (they beat environment variables)
    #[command(subcommand)]
    Key(KeyCmd),
    /// Download the latest rig (verified), used on the next start. Checks run automatically
    /// at most once a day.
    Update {
        /// Only report the latest release
        #[arg(long, conflicts_with_all = ["off", "on", "rollback"])]
        check: bool,
        /// Stop automatic checks (manual `rig update` still works)
        #[arg(long, conflicts_with_all = ["on", "rollback"])]
        off: bool,
        /// Turn automatic checks back on
        #[arg(long, conflicts_with_all = ["rollback"])]
        on: bool,
        /// Go back to the version before the last update
        #[arg(long)]
        rollback: bool,
    },
    /// List providers, default models and credential status
    Models,
    /// Check the local setup
    Doctor,
}

#[derive(Subcommand)]
pub enum ProfileCmd {
    /// List profiles; * marks the one in use
    #[command(alias = "ls")]
    List,
    /// Create a profile
    Create {
        name: String,
        /// Switch to it as well
        #[arg(long = "use")]
        switch: bool,
    },
    /// Use this profile from now on (`--profile` and $RIG_PROFILE still win for one run)
    Use { name: String },
    /// Delete a profile with its logins, keys, settings and sessions
    #[command(alias = "rm")]
    Remove {
        name: String,
        /// Do not ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

#[derive(Subcommand)]
pub enum KeyCmd {
    /// Save a provider's API key; reads it without echo, or from stdin when piped
    Set { provider: String },
    /// Forget a provider's saved key
    #[command(alias = "rm")]
    Remove { provider: String },
    /// Show saved keys (masked) and where every provider's credential comes from
    #[command(alias = "ls")]
    List,
}

/// The permission level a run starts with when no `--auto` is given. `rigd` (a link to this same
/// binary) starts with full access, so it never asks before running anything.
fn default_autonomy(argv0: Option<&std::ffi::OsStr>) -> Option<&'static str> {
    let name = std::path::Path::new(argv0?).file_name()?;
    (name == "rigd").then_some("high")
}

/// Starts `bin` in place of this process, keeping the name it was started under (so `rigd` stays
/// `rigd`). Returns only if starting it failed; this process then carries on.
fn restart_as(bin: &std::path::Path) {
    use std::os::unix::process::CommandExt;
    let argv0 = std::env::args_os().next().unwrap_or_else(|| "rig".into());
    let _ = std::process::Command::new(bin)
        .arg0(argv0)
        .args(std::env::args_os().skip(1))
        .exec();
}

fn main() {
    let mut cli = Cli::parse();
    if cli.opts.auto.is_none() {
        cli.opts.auto = default_autonomy(std::env::args_os().next().as_deref()).map(String::from);
    }
    // Profile management must work even when the profile in use is broken or missing.
    if let Some(Cmd::Profile(cmd)) = &cli.cmd {
        std::process::exit(subcommands::profile(cmd, cli.opts.profile.as_deref()));
    }
    let (base, _) = config::Paths::base();
    let exe = std::env::current_exe().and_then(std::fs::canonicalize).ok();
    if let Some(Cmd::Update {
        check,
        off,
        on,
        rollback,
    }) = &cli.cmd
    {
        let action = match (*check, *off, *on, *rollback) {
            (_, _, _, true) => update::Action::Rollback,
            (true, _, _, _) => update::Action::Check,
            (_, true, _, _) => update::Action::Off,
            (_, _, true, _) => update::Action::On,
            _ => update::Action::Install,
        };
        let Some(exe) = exe else {
            eprintln!("error: cannot find the rig binary to update");
            std::process::exit(1);
        };
        std::process::exit(update::command(&base, &exe, action));
    }
    // A verified update staged by an earlier run is swapped in before anything else starts. Then
    // this run continues as the new binary.
    if let Some(exe) = &exe {
        if let Some(new_bin) = update::apply_staged(&base, exe) {
            restart_as(&new_bin);
        }
        update::check_in_background(&base, exe);
    }
    let app = match App::load(cli.opts.profile.as_deref()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let first_message = Some(cli.prompt.join(" ")).filter(|p| !p.trim().is_empty());
    let opts = RunOpts {
        model: cli.opts.model,
        effort: cli.opts.effort,
        auto: cli.opts.auto,
        cont: cli.opts.cont,
        resume: cli.opts.resume,
        first_message,
    };
    let code = match cli.cmd {
        None => tui::run(app, opts),
        Some(Cmd::Ask { prompt, json }) => subcommands::ask(&app, &opts, &prompt, json),
        Some(Cmd::Login { provider }) => subcommands::login(&app, &provider),
        Some(Cmd::Logout) => subcommands::logout(&app),
        Some(Cmd::Models) => subcommands::models(&app),
        Some(Cmd::Doctor) => subcommands::doctor(&app),
        Some(Cmd::Key(cmd)) => subcommands::key(app, &cmd),
        Some(Cmd::Profile(_)) => 0,
        Some(Cmd::Update { .. }) => unreachable!("handled before the app starts"),
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests;
