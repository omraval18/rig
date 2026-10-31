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
mod runtime;
mod screen;
mod slash;
mod state;
mod style;
mod subcommands;
mod transcript;
mod tui;
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

fn main() {
    let cli = Cli::parse();
    // Profile management must work even when the profile in use is broken or missing.
    if let Some(Cmd::Profile(cmd)) = &cli.cmd {
        std::process::exit(subcommands::profile(cmd, cli.opts.profile.as_deref()));
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
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests;
