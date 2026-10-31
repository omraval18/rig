//! Autonomy levels and the allow / ask / deny decision.
//!
//! The agent declares a `risk` for each shell command and the session's autonomy
//! level decides what runs unattended.
//! We never trust the declared level alone: `risk_floor` raises it for
//! commands that are dangerous regardless of what the model claims.

use crate::sync::MutexExt;
use crate::tool::{Access, Risk};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Autonomy {
    /// Every edit and command asks first; reads and read-only commands run.
    Ask,
    /// Read-only tools and read-only commands. Edits are refused, not asked.
    ReadOnly,
    /// Plus file edits and low-risk commands.
    Low,
    /// Plus reversible workspace changes: installs, local commits, builds.
    Medium,
    /// Everything, including network writes and destructive commands.
    High,
}

impl Autonomy {
    /// Accepts the five levels, plus `auto` for low and `full-access` for high.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "ask" => Autonomy::Ask,
            "read-only" | "readonly" | "off" => Autonomy::ReadOnly,
            "low" | "auto" => Autonomy::Low,
            "medium" => Autonomy::Medium,
            "high" | "full-access" => Autonomy::High,
            _ => return None,
        })
    }

    pub const ALL: [Autonomy; 5] = [
        Autonomy::Ask,
        Autonomy::ReadOnly,
        Autonomy::Low,
        Autonomy::Medium,
        Autonomy::High,
    ];

    pub fn describe(self) -> &'static str {
        match self {
            Autonomy::Ask => "approve every edit and command yourself",
            Autonomy::ReadOnly => "read and search only; edits are refused",
            Autonomy::Low => "edit files and run low-risk commands (fx: auto)",
            Autonomy::Medium => "also installs, builds and local commits",
            Autonomy::High => "everything, including pushes and deletes",
        }
    }

    fn to_u8(self) -> u8 {
        Self::ALL.iter().position(|a| *a == self).unwrap_or(2) as u8
    }

    fn from_u8(n: u8) -> Self {
        Self::ALL.get(n as usize).copied().unwrap_or(Autonomy::Low)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Autonomy::Ask => "ask",
            Autonomy::ReadOnly => "read-only",
            Autonomy::Low => "low",
            Autonomy::Medium => "medium",
            Autonomy::High => "high",
        }
    }

    fn max_risk(self) -> Option<Risk> {
        match self {
            Autonomy::Ask | Autonomy::ReadOnly => None,
            Autonomy::Low => Some(Risk::Low),
            Autonomy::Medium => Some(Risk::Medium),
            Autonomy::High => Some(Risk::High),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Needs the user's approval. Treated as a denial when nobody can answer.
    Ask(String),
    Deny(String),
}

/// The user's answer to an approval prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Approval {
    No,
    Once,
    /// Allow this kind of call from now on.
    Always,
}

/// Cheap to clone; clones share state, so changing the level from the UI applies
/// to tool calls the agent makes afterwards, even mid-turn.
#[derive(Clone, Debug)]
pub struct Policy {
    level: Arc<AtomicU8>,
    allow: Arc<Mutex<Vec<String>>>,
}

impl Policy {
    pub fn new(autonomy: Autonomy) -> Self {
        Self {
            level: Arc::new(AtomicU8::new(autonomy.to_u8())),
            allow: Arc::default(),
        }
    }

    pub fn autonomy(&self) -> Autonomy {
        Autonomy::from_u8(self.level.load(Ordering::Relaxed))
    }

    pub fn set(&self, a: Autonomy) {
        self.level.store(a.to_u8(), Ordering::Relaxed);
    }

    pub fn allowlist(&self) -> Vec<String> {
        self.allow.locked().clone()
    }

    pub fn set_allowlist(&self, entries: Vec<String>) {
        *self.allow.locked() = entries;
    }

    /// Remember that the user chose "always" for calls like `access`. Returns the stored rule.
    pub fn remember(&self, access: &Access) -> Option<String> {
        let key = allow_key(access)?;
        let mut a = self.allow.locked();
        if !a.contains(&key) {
            a.push(key.clone());
        }
        Some(key)
    }

    fn allowed(&self, access: &Access) -> bool {
        allow_key(access).is_some_and(|k| self.allow.locked().contains(&k))
    }

    pub fn decide(&self, access: &Access) -> Decision {
        let autonomy = self.autonomy();
        match access {
            Access::Read | Access::Network => Decision::Allow,
            Access::Write => match autonomy {
                Autonomy::ReadOnly => {
                    Decision::Deny("editing files is disabled in read-only mode".into())
                }
                _ if self.allowed(access) => Decision::Allow,
                Autonomy::Ask => Decision::Ask("ask mode: edits need your approval".into()),
                _ => Decision::Allow,
            },
            Access::Exec { risk, command } => {
                if matches!(autonomy, Autonomy::ReadOnly | Autonomy::Ask)
                    && is_read_only_command(command)
                {
                    return Decision::Allow;
                }
                if autonomy == Autonomy::ReadOnly {
                    return Decision::Deny("only read-only commands run in read-only mode".into());
                }
                if self.allowed(access) {
                    return Decision::Allow;
                }
                if autonomy == Autonomy::Ask {
                    return Decision::Ask("ask mode: commands need your approval".into());
                }
                let effective = (*risk).max(risk_floor(command));
                match autonomy.max_risk() {
                    Some(max) if effective <= max => Decision::Allow,
                    _ => Decision::Ask(format!(
                        "{} risk command exceeds autonomy level '{}'",
                        risk_name(effective),
                        autonomy.as_str()
                    )),
                }
            }
        }
    }
}

/// The rule an "always allow" answer would store for this call.
pub fn rule_for(access: &Access) -> Option<String> {
    allow_key(access)
}

/// Rule key for "always allow": the program and subcommand of a simple command, or edits as a class.
fn allow_key(access: &Access) -> Option<String> {
    match access {
        Access::Write => Some("edit".into()),
        Access::Exec { command, .. } => {
            if command.contains(['&', ';', '|', '`', '>', '<', '\n']) || command.contains("$(") {
                return None;
            }
            let words: Vec<&str> = command.split_whitespace().take(2).collect();
            (!words.is_empty()).then(|| format!("run {}", words.join(" ")))
        }
        _ => None,
    }
}

fn risk_name(r: Risk) -> &'static str {
    match r {
        Risk::Low => "low",
        Risk::Medium => "medium",
        Risk::High => "high",
    }
}

/// Lower bound on a command's risk, whatever the model declared.
pub fn risk_floor(command: &str) -> Risk {
    let c = format!(
        " {} ",
        command.replace(['\n', ';', '&', '|', '(', ')'], " ")
    );
    let has = |needle: &str| c.contains(needle);

    let high = [
        " sudo ",
        " doas ",
        " mkfs",
        " dd if=",
        " git push",
        " git reset --hard",
        " git clean",
        " chmod -R",
        " chown -R",
        " rm -rf",
        " rm -fr",
        " rm -r ",
        " --force",
        " shutdown ",
        " reboot ",
        " kill -9",
        " killall ",
        " npm publish",
        " cargo publish",
        " docker rm",
        " kubectl delete",
    ];
    let pipes_to_shell =
        command.contains("| sh") || command.contains("| bash") || command.contains("|sh");
    if pipes_to_shell || high.iter().any(|p| has(p)) {
        return Risk::High;
    }
    let medium = [
        " rm ",
        " mv ",
        " git commit",
        " git checkout",
        " git switch",
        " git merge",
        " git rebase",
        " git stash",
        " npm install",
        " npm i ",
        " pnpm install",
        " yarn add",
        " pip install",
        " cargo install",
        " brew ",
        " apt ",
        " apt-get ",
        " chmod ",
        " chown ",
        " ln -s",
    ];
    if medium.iter().any(|p| has(p)) {
        return Risk::Medium;
    }
    Risk::Low
}

/// Conservative allowlist for read-only mode. Anything with redirection,
/// command substitution or an unknown program is rejected.
pub fn is_read_only_command(command: &str) -> bool {
    if command.contains(['>', '`']) || command.contains("$(") {
        return false;
    }
    const OK: &[&str] = &[
        "ls", "cat", "pwd", "head", "tail", "wc", "rg", "grep", "stat", "file", "tree", "which",
        "echo", "date", "uname", "whoami", "env", "printenv", "du", "df", "sort", "uniq", "cut",
        "tr", "basename", "dirname", "realpath", "diff", "jq", "true", "false", "test", "[",
    ];
    for seg in command
        .split(['|', ';', '\n'])
        .flat_map(|s| s.split("&&"))
        .flat_map(|s| s.split("||"))
    {
        let mut words = seg.split_whitespace();
        let Some(prog) = words.next() else { continue };
        let ok = match prog {
            "git" => matches!(
                words.next(),
                Some(
                    "status"
                        | "log"
                        | "diff"
                        | "show"
                        | "branch"
                        | "rev-parse"
                        | "ls-files"
                        | "blame"
                        | "remote"
                )
            ),
            "find" => !seg.contains("-exec") && !seg.contains("-delete") && !seg.contains("-ok"),
            "sed" => !seg.contains(" -i"),
            p => OK.contains(&p),
        };
        if !ok {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests;
