//! Skills: folders holding a `SKILL.md` with `name` and `description` frontmatter.
//!
//! Only names and descriptions are shown to the model up front. The full
//! instructions are read when a skill is activated, by the user (`$name`) or by
//! the model (the `use_skill` tool), so installed skills cost little context.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Absolute path of the SKILL.md file.
    pub path: PathBuf,
    /// Where it was found, e.g. "project" or "user".
    pub scope: &'static str,
}

const MAX_FRONTMATTER: usize = 64 * 1024;
/// Skill folders read in a project and in the home directory, most specific first.
const SKILL_DIRS: &[&str] = &[".rig/skills", ".agents/skills", ".claude/skills"];

/// Skill directories to scan, most specific first: project folders from `cwd` up to the
/// repository root, then the user's home.
pub fn roots(cwd: &Path, home: Option<&Path>) -> Vec<(PathBuf, &'static str)> {
    let mut out = Vec::new();
    let repo_root = cwd
        .ancestors()
        .find(|d| d.join(".git").exists())
        .unwrap_or(cwd);
    for dir in cwd.ancestors() {
        for r in SKILL_DIRS {
            out.push((dir.join(r), "project"));
        }
        if dir == repo_root {
            break;
        }
    }
    if let Some(h) = home {
        for r in SKILL_DIRS {
            out.push((h.join(r), "user"));
        }
    }
    out
}

/// All skills, deduplicated by name; the first (most specific) root wins.
pub fn discover(cwd: &Path, home: Option<&Path>) -> Vec<Skill> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (root, scope) in roots(cwd, home) {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for dir in dirs {
            let file = dir.join("SKILL.md");
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let fallback = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some(skill) = parse(&text, &fallback, file, scope)
                && seen.insert(skill.name.clone())
            {
                out.push(skill);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
}

/// Split `---\nkey: value\n---\nbody`. Returns (frontmatter, body) or None when there is none.
fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let head_limit = rest.len().min(MAX_FRONTMATTER);
    let end = rest[..head_limit].find("\n---")?;
    let after = &rest[end + 4..];
    let body = after
        .strip_prefix('\n')
        .or_else(|| after.strip_prefix("\r\n"))
        .unwrap_or(after);
    Some((&rest[..end], body))
}

/// Minimal YAML: `key: value`, quoted values, and `>` / `|` block scalars.
fn frontmatter_field(front: &str, key: &str) -> Option<String> {
    let mut lines = front.lines().peekable();
    while let Some(line) = lines.next() {
        let Some(value) = line.strip_prefix(key).and_then(|r| r.strip_prefix(':')) else {
            continue;
        };
        let value = value.trim();
        if matches!(value, ">" | "|" | ">-" | "|-") {
            let mut parts = Vec::new();
            while let Some(next) =
                lines.next_if(|l| l.starts_with([' ', '\t']) || l.trim().is_empty())
            {
                parts.push(next.trim().to_string());
            }
            return Some(parts.join(" ").trim().to_string());
        }
        let unquoted = value.trim_matches(|c| c == '"' || c == '\'');
        return Some(unquoted.to_string());
    }
    None
}

pub fn parse(text: &str, fallback_name: &str, path: PathBuf, scope: &'static str) -> Option<Skill> {
    let (front, body) = split_frontmatter(text).map_or(("", text), |(f, b)| (f, b));
    let name = frontmatter_field(front, "name")
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| fallback_name.to_string());
    if !valid_name(&name) {
        return None;
    }
    let description = frontmatter_field(front, "description")
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| {
            body.lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'))
                .unwrap_or("")
                .chars()
                .take(160)
                .collect()
        });
    Some(Skill {
        name,
        description,
        path,
        scope,
    })
}

/// Full instructions (the SKILL.md body without frontmatter).
pub fn body(skill: &Skill) -> std::io::Result<String> {
    let text = std::fs::read_to_string(&skill.path)?;
    Ok(split_frontmatter(&text).map_or(text.clone(), |(_, b)| b.to_string()))
}

/// What the model reads when a skill is activated.
pub fn activation_text(skill: &Skill) -> std::io::Result<String> {
    let dir = skill
        .path
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    Ok(format!(
        "<rig-context>\nSkill \"{}\" is active. Its files live in {dir}. Follow these instructions:\n\n{}\n</rig-context>",
        skill.name,
        body(skill)?.trim()
    ))
}

/// Roughly 2k tokens: enough to route a task to a skill without taxing every request.
const CATALOG_BUDGET: usize = 8_000;

/// Catalog section for the session reminder: names and short descriptions, within a budget.
/// Skills arrive project-first, so when the budget runs out the ones cut are the user-level extras.
pub fn catalog_text(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let head = "# Skills\nAvailable skills. Activate one with the use_skill tool when a task matches it; the user can also invoke one with $name.\n";
    let line = |k: &Skill, max: usize| {
        let d: String = k.description.chars().take(max).collect();
        format!(
            "- {}: {}{}\n",
            k.name,
            d.trim(),
            if k.description.chars().count() > max {
                "…"
            } else {
                ""
            }
        )
    };
    for max in [200, 100, 60] {
        let body: String = skills.iter().map(|k| line(k, max)).collect();
        if head.len() + body.len() <= CATALOG_BUDGET {
            return format!("{head}{body}");
        }
    }
    let mut out = String::from(head);
    let mut shown = 0;
    for k in skills {
        let l = line(k, 60);
        if out.len() + l.len() > CATALOG_BUDGET {
            break;
        }
        out.push_str(&l);
        shown += 1;
    }
    out.push_str(&format!("- …and {} more skills not listed here; the user can find them with $ and you can load one by name with the Skill tool.\n", skills.len() - shown));
    out
}

/// A starter SKILL.md for `/skills create`.
pub fn template(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Describe when this skill should be used.\n---\n\n# {name}\n\nWrite the instructions the agent should follow.\n"
    )
}

#[cfg(test)]
mod tests;
