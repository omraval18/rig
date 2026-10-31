//! `/skills`.

use super::first_line;
use crate::picker::{PickItem, PickKind};
use crate::tui::Tui;
use rig_core::skills;
use rig_core::sync::RwLockExt;

const USAGE: &str = "Usage: /skills [list|show <name>|create <name>|path]";

impl Tui {
    pub(super) fn refresh_skills(&mut self) {
        self.skills = self.app.discover_skills();
        *self.shared.skills.write_locked() = self.skills.clone();
    }

    pub(super) fn cmd_skills(&mut self, arg: &str) {
        let (sub, rest) = arg
            .split_once(char::is_whitespace)
            .map_or((arg, ""), |(a, b)| (a, b.trim()));
        match sub {
            "" => self.pick_skill(),
            "list" => self.list_skills(),
            "show" => self.show_skill(rest),
            "create" => self.create_skill(rest.trim_start_matches('$')),
            "path" => self.skill_paths(),
            _ => self.say(USAGE),
        }
    }

    fn pick_skill(&mut self) {
        self.refresh_skills();
        if self.skills.is_empty() {
            return self.say("No skills found. Create one with /skills create <name>; see /skills path for where they are read from.");
        }
        let items = self
            .skills
            .iter()
            .map(|s| PickItem::labeled(&s.name).detail(first_line(&s.description, 80)))
            .collect();
        let title = "Skills (Enter inserts $name into your prompt)";
        self.open_picker(PickKind::Skills, title, items);
    }

    fn list_skills(&mut self) {
        self.refresh_skills();
        if self.skills.is_empty() {
            return self.say("No skills found.");
        }
        let rows: Vec<String> = self
            .skills
            .iter()
            .map(|s| {
                let description = first_line(&s.description, 70);
                format!("  ${:<22} {description}  [{}]", s.name, s.scope)
            })
            .collect();
        self.say(&format!("Skills:\n{}", rows.join("\n")));
    }

    fn show_skill(&mut self, name: &str) {
        let name = name.trim_start_matches('$');
        let Some(skill) = self.skills.iter().find(|s| s.name == name).cloned() else {
            return self.error(&format!("no skill named '{name}'"));
        };
        let body = skills::body(&skill).unwrap_or_default();
        let shown: Vec<&str> = body.lines().take(40).collect();
        self.say(&format!(
            "${} — {}\n{}\n\n{}",
            skill.name,
            skill.description,
            skill.path.display(),
            shown.join("\n")
        ));
    }

    fn create_skill(&mut self, name: &str) {
        let valid = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
        if !valid {
            return self.say("Usage: /skills create <name>  (letters, digits, - and _)");
        }
        let dir = self.app.cwd.join(".rig").join("skills").join(name);
        let file = dir.join("SKILL.md");
        if file.exists() {
            return self.error(&format!("{} already exists", file.display()));
        }
        let written = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&file, skills::template(name)));
        match written {
            Ok(()) => {
                self.refresh_skills();
                let path = file.display();
                self.say(&format!(
                    "Created {path}. Edit it, then use it with ${name}."
                ));
            }
            Err(e) => self.error(&e.to_string()),
        }
    }

    fn skill_paths(&mut self) {
        let rows: Vec<String> = skills::roots(&self.app.cwd, self.app.paths.home.as_deref())
            .iter()
            .map(|(path, scope)| {
                let absent = if path.is_dir() { "" } else { "  (absent)" };
                format!("  [{scope}] {}{absent}", path.display())
            })
            .collect();
        self.say(&format!(
            "Skill folders, most specific first:\n{}",
            rows.join("\n")
        ));
    }
}
