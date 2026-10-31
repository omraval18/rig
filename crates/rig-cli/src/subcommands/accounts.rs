//! `rig profile ...` and `rig key ...`.

use crate::app::App;
use crate::config::Paths;
use crate::keys::masked;
use crate::{KeyCmd, ProfileCmd, profile};
use std::io::{IsTerminal, Write};

pub fn profile(cmd: &ProfileCmd, flag: Option<&str>) -> i32 {
    let (base, _) = Paths::base();
    let result = match cmd {
        ProfileCmd::List => {
            let in_use = Paths::named_profile(flag).unwrap_or_else(|| profile::active(&base));
            for name in profile::list(&base) {
                let mark = if name == in_use { "*" } else { " " };
                println!(
                    "{mark} {name:<16} {}",
                    profile::describe(&base, &name)
                );
            }
            Ok(())
        }
        ProfileCmd::Create { name, switch } => profile::create(&base, name).and_then(|dir| {
            println!("Created profile '{name}' in {}", dir.display());
            if *switch {
                profile::set_active(&base, name)?;
                println!("Now using '{name}'.");
            }
            println!(
                "Sign in with: rig --profile {name} login codex   or   rig --profile {name} key set <provider>"
            );
            Ok(())
        }),
        ProfileCmd::Use { name } => profile::set_active(&base, name).map(|()| {
            println!("Now using profile '{name}'.");
        }),
        ProfileCmd::Remove { name, yes } => {
            if !yes && !confirm(&format!(
                "Delete profile '{name}' with its logins, keys, settings and sessions? [y/N] "
            )) {
                println!("Kept '{name}'.");
                return 0;
            }
            profile::remove(&base, name).map(|dir| println!("Removed {}", dir.display()))
        }
    };
    report(result)
}

pub fn key(mut app: App, cmd: &KeyCmd) -> i32 {
    let file = app.paths.keys_file();
    let profile = app.paths.profile.clone();
    let result = match cmd {
        KeyCmd::Set { provider } => app
            .provider_spec(provider)
            .and_then(|spec| {
                if spec.chatgpt_plan {
                    return Err(format!(
                        "'{provider}' signs in with your ChatGPT plan: rig login codex"
                    ));
                }
                read_secret(&format!("API key for {provider} (profile {profile}): "))
            })
            .and_then(|k| {
                if k.trim().is_empty() {
                    return Err("no key entered; nothing saved".into());
                }
                app.keys.set(provider, &k);
                app.keys.save(&file)?;
                println!(
                    "Saved {} for {provider} in profile '{profile}'.",
                    masked(k.trim())
                );
                Ok(())
            }),
        KeyCmd::Remove { provider } => {
            if app.keys.remove(provider) {
                app.keys
                    .save(&file)
                    .map(|()| println!("Removed the {provider} key from profile '{profile}'."))
            } else {
                Err(format!(
                    "profile '{profile}' has no saved key for {provider}"
                ))
            }
        }
        KeyCmd::List => {
            println!("Profile '{profile}' ({})", file.display());
            for spec in app.cfg.providers() {
                let cred = app.credential(&spec);
                let shown = match app.keys.get(&spec.name) {
                    Some(k) if cred == crate::app::Credential::Saved => {
                        format!("{} {}", cred.describe(), masked(k))
                    }
                    _ => cred.describe(),
                };
                println!("  {:<12} {shown}", spec.name);
            }
            Ok(())
        }
    };
    report(result)
}

fn report(r: Result<(), String>) -> i32 {
    match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn confirm(question: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    eprint!("{question}");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).is_ok() && line.trim().eq_ignore_ascii_case("y")
}

/// A line from stdin without echoing it. Piped input is read as is, so `pbpaste | rig key set x` works.
fn read_secret(prompt: &str) -> Result<String, String> {
    let mut line = String::new();
    if !std::io::stdin().is_terminal() {
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        return Ok(line);
    }
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    // Raw mode turns echo off and turns Ctrl+C into a key, so the terminal is always restored.
    crossterm::terminal::enable_raw_mode().map_err(|e| e.to_string())?;
    let result = loop {
        match event::read() {
            Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => match k.code {
                KeyCode::Enter => break Ok(line),
                KeyCode::Esc => break Err("cancelled".to_string()),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    break Err("cancelled".to_string());
                }
                KeyCode::Backspace => {
                    line.pop();
                }
                KeyCode::Char(c) => line.push(c),
                _ => {}
            },
            Ok(Event::Paste(p)) => line.push_str(&p),
            Ok(_) => {}
            Err(e) => break Err(e.to_string()),
        }
    };
    let _ = crossterm::terminal::disable_raw_mode();
    eprintln!();
    result
}
