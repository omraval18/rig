//! Non-interactive subcommands.

use crate::app::{App, Credential, RunOpts};
use crate::desktop;
use crate::models::Models;
use crate::plain::PlainUi;
use rig_core::message::{Block, Message, Role};
use rig_provider::chatgpt;
use serde_json::json;
use std::io::{IsTerminal, Read};
use std::process::Command;

mod accounts;

pub use accounts::{key, profile};

pub fn ask(app: &App, opts: &RunOpts, words: &[String], as_json: bool) -> i32 {
    let mut prompt = words.join(" ");
    if prompt.trim().is_empty() || prompt.trim() == "-" {
        prompt.clear();
        if std::io::stdin().is_terminal() {
            eprintln!("error: no prompt given. Usage: rig ask \"what changed in this repo?\"");
            return 1;
        }
        let _ = std::io::stdin().read_to_string(&mut prompt);
    }
    if prompt.trim().is_empty() {
        eprintln!("error: empty prompt");
        return 1;
    }
    let started = match app.start(opts, false) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    let mut agent = started.agent;
    let mut term = PlainUi::new();
    term.session = Some(started.session);
    if as_json {
        term.capture = Some(String::new());
    }
    // `@file` and `$skill` work in scripts too.
    let ex = rig_core::expand::expand(
        &prompt,
        &app.cwd,
        app.paths.home.as_deref(),
        &started.skills,
    );
    for m in &ex.missing {
        eprintln!("warning: {m} not found; sent as plain text");
    }
    let mut blocks = vec![Block::Text {
        text: prompt.clone(),
    }];
    blocks.extend(ex.reminders.iter().map(|r| Block::Text { text: r.clone() }));
    blocks.extend(ex.images.iter().cloned());
    let result = agent.run_turn(
        Message {
            role: Role::User,
            blocks,
        },
        &mut term,
    );
    let usage = term.usage;
    let session_id = agent.ctx.session_id.clone();
    let model = format!("{}/{}", agent.provider.name(), agent.provider.model());
    match result {
        Ok(()) => {
            if as_json {
                let out = json!({
                    "final_output": term.capture.take().unwrap_or_default(),
                    "session_id": session_id,
                    "model": model,
                    "usage": {"input_tokens": usage.input, "output_tokens": usage.output, "cached_tokens": usage.cached},
                });
                println!("{out}");
            } else {
                term.finish();
            }
            0
        }
        Err(e) => {
            term.finish();
            term.error(&e.to_string());
            1
        }
    }
}

pub fn login(app: App, provider: &str) -> i32 {
    match provider {
        "codex" | "chatgpt" => {
            let path = app.paths.chatgpt_auth_file();
            let plan = match chatgpt::LoginPlan::new() {
                Ok(plan) => plan,
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            };
            let announce = |url: &str| {
                eprintln!(
                    "Opening your browser to sign in with ChatGPT.\nIf it does not open, visit:\n\n  {url}\n\nWaiting for you to finish (up to 5 minutes)..."
                );
                desktop::open_url(url);
            };
            match chatgpt::login(&path, &plan, &announce) {
                Ok(()) => {
                    println!(
                        "Signed in to profile '{}'. {}",
                        app.paths.profile,
                        chatgpt::status(&path).unwrap_or_default()
                    );
                    println!("Tokens stored in {}", path.display());
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        other => key(
            app,
            &crate::KeyCmd::Set {
                provider: other.into(),
            },
        ),
    }
}

pub fn logout(app: &App) -> i32 {
    let path = app.paths.chatgpt_auth_file();
    match std::fs::remove_file(&path) {
        Ok(()) => {
            println!(
                "Signed out of profile '{}': removed {}",
                app.paths.profile,
                path.display()
            );
            0
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("Profile '{}' is not signed in.", app.paths.profile);
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

/// Every connected provider's models, one `provider/model` per line, ready for `rig -m`.
pub fn models(app: &App, query: &[String], refresh: bool) -> i32 {
    let mut known = Models::load(&app.paths.models_file());
    let mut specs = app.cfg.providers();
    specs.retain(|s| app.has_credentials(s));
    let mut fetches = Vec::new();
    for spec in &specs {
        if !refresh && !known.is_stale(spec) {
            continue;
        }
        match app.model_lister(spec) {
            Ok(lister) => {
                let seq = known.begin(spec);
                fetches.push((spec, seq, std::thread::spawn(lister)));
            }
            Err(e) => eprintln!("{}: {e}", spec.name),
        }
    }
    for (spec, seq, thread) in fetches {
        let result = thread.join().unwrap_or_else(|_| Err("failed".into()));
        // A local server that is not running is not worth a warning.
        if let Some(Err(e)) = known.finish(&spec.name, seq, result)
            && !spec.keyless
        {
            eprintln!("{}: {e}", spec.name);
        }
    }
    let words: Vec<String> = query.iter().map(|w| w.to_lowercase()).collect();
    let mut shown = 0;
    for spec in &specs {
        for m in known.of(spec).iter().filter(|m| !m.hidden) {
            let id = format!("{}/{}", spec.name, m.id);
            let detail = crate::slash::model::model_detail(spec, m);
            let hay = format!("{id} {detail}").to_lowercase();
            if words.iter().all(|w| hay.contains(w)) {
                println!("{}", format!("{id:<44} {detail}").trim_end());
                shown += 1;
            }
        }
    }
    if shown == 0 {
        eprintln!("no models found; `rig providers` shows what is connected");
        return 1;
    }
    0
}

pub fn providers(app: &App) -> i32 {
    println!("Profile '{}'\n", app.paths.profile);
    println!(
        "{:<12} {:<10} {:<28} AUTH",
        "PROVIDER", "WIRE", "DEFAULT MODEL"
    );
    for s in app.cfg.providers() {
        let cred = app.credential(&s);
        let auth = match &cred {
            Credential::Plan { signed_in: true } => {
                match chatgpt::status(&app.paths.chatgpt_auth_file()) {
                    Ok(st) => format!("ok ({st})"),
                    Err(e) => e.to_string(),
                }
            }
            Credential::Plan { signed_in: false } => "not signed in: rig login codex".into(),
            Credential::Missing(var) => format!("missing: rig key set {} (or ${var})", s.name),
            Credential::Env(_) => format!("ok ({})", cred.describe()),
            other => other.describe(),
        };
        println!(
            "{:<12} {:<10} {:<28} {}",
            s.name,
            format!("{:?}", s.kind).to_lowercase(),
            app.default_model_of(&s).unwrap_or_else(|| "-".into()),
            auth
        );
    }
    println!("\nUse: rig -m <provider>/<model>, e.g. rig -m anthropic/claude-sonnet-5-5");
    0
}

pub fn doctor(app: &App, model: Option<&str>) -> i32 {
    let mut bad = 0;
    let mut check = |ok: bool, what: String| {
        println!("{} {what}", if ok { "✓" } else { "✗" });
        if !ok {
            bad += 1;
        }
    };
    check(
        true,
        format!(
            "rig {} on {} {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    );
    check(
        app.paths.root.is_dir() || std::fs::create_dir_all(&app.paths.root).is_ok(),
        format!(
            "profile '{}' in {}",
            app.paths.profile,
            app.paths.root.display()
        ),
    );
    for f in app.paths.config_files(&app.cwd) {
        println!(
            "  config {} {}",
            f.display(),
            if f.is_file() { "(loaded)" } else { "(absent)" }
        );
    }
    check(
        Command::new("sh")
            .arg("-c")
            .arg("true")
            .status()
            .is_ok_and(|s| s.success()),
        "sh available for Execute".into(),
    );
    if !Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|s| s.status.success())
    {
        println!("  git not found (optional; install Git for repository context)");
    }
    match app.resolve_model(model) {
        Ok((s, m)) => {
            check(true, format!("selected model {}/{m}", s.name));
            let credential = app.credential(&s);
            let hint = match &credential {
                Credential::Plan { signed_in: false } => "run `rig login codex`".to_string(),
                Credential::Missing(_) => {
                    format!("run `rig key set {}` or configure its key", s.name)
                }
                _ => credential.describe(),
            };
            check(credential.usable(), format!("provider credentials: {hint}"));
        }
        Err(e) => check(false, format!("default model: {e}")),
    }
    match chatgpt::status(&app.paths.chatgpt_auth_file()) {
        Ok(st) => check(true, st),
        Err(_) => println!("  ChatGPT: not signed in (rig login codex)"),
    }
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        println!(
            "  terminal: stdin or stdout is not a terminal; run `rig` in an interactive terminal for the UI"
        );
    } else if std::env::var("TERM").map_or(true, |term| term.is_empty() || term == "dumb") {
        println!("  terminal: TERM is unset or dumb; use a terminal with cursor and color support");
    } else {
        println!("  terminal: interactive (capabilities not tested)");
    }
    if cfg!(target_os = "linux") && !command_on_path("wl-paste") && !command_on_path("xclip") {
        println!("  image clipboard: optional; install wl-clipboard or xclip to paste images");
    }
    if !command_on_path("rig") {
        println!("  PATH: rig not found; add its install directory to PATH");
    }
    println!(
        "  Provider connectivity and model availability were not tested (no network requests)."
    );
    if bad == 0 { 0 } else { 1 }
}

fn command_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}
