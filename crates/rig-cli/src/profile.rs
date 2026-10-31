//! Profiles: separate sets of logins, API keys and settings under one `~/.rig`.
//!
//! The default profile is `~/.rig` itself, so a setup from before profiles keeps working
//! untouched. Every other profile is a directory `~/.rig/profiles/<name>/` holding its own
//! `auth.json` (ChatGPT login), `keys.json`, `config.toml`, `state.json`, allowlist, history and
//! sessions. `~/.rig/config.toml` and `~/.rig/AGENTS.md` are shared by all of them.

use std::path::{Path, PathBuf};

pub const DEFAULT: &str = "default";

/// Holds the name chosen with `rig profile use`; absent means the default profile.
fn active_file(base: &Path) -> PathBuf {
    base.join("profile")
}

fn profiles_dir(base: &Path) -> PathBuf {
    base.join("profiles")
}

pub fn dir(base: &Path, name: &str) -> PathBuf {
    if name == DEFAULT {
        base.to_path_buf()
    } else {
        profiles_dir(base).join(name)
    }
}

/// Short, and safe as a directory name.
pub fn validate(name: &str) -> Result<(), String> {
    let ok_chars = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if name.is_empty() || name.len() > 32 || !ok_chars || name.starts_with('-') {
        return Err(format!(
            "'{name}' is not a valid profile name: use up to 32 letters, digits, '-' or '_'"
        ));
    }
    Ok(())
}

pub fn exists(base: &Path, name: &str) -> bool {
    name == DEFAULT || (validate(name).is_ok() && dir(base, name).is_dir())
}

pub fn check_exists(base: &Path, name: &str) -> Result<(), String> {
    validate(name)?;
    if exists(base, name) {
        Ok(())
    } else {
        Err(format!(
            "no profile named '{name}'. Create it with: rig profile create {name}"
        ))
    }
}

/// The profile chosen with `rig profile use`, or the default one.
pub fn active(base: &Path) -> String {
    std::fs::read_to_string(active_file(base))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|n| exists(base, n))
        .unwrap_or_else(|| DEFAULT.to_string())
}

pub fn set_active(base: &Path, name: &str) -> Result<(), String> {
    check_exists(base, name)?;
    let file = active_file(base);
    let r = if name == DEFAULT {
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        rig_core::fsutil::write_atomic(&file, format!("{name}\n").as_bytes(), 0o644).map(drop)
    };
    r.map_err(|e| format!("cannot write {}: {e}", file.display()))
}

/// Every profile, the default one first.
pub fn list(base: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(profiles_dir(base))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| validate(n).is_ok() && n != DEFAULT)
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.insert(0, DEFAULT.to_string());
    names
}

pub fn create(base: &Path, name: &str) -> Result<PathBuf, String> {
    validate(name)?;
    if exists(base, name) {
        return Err(format!("profile '{name}' already exists"));
    }
    let d = dir(base, name);
    std::fs::create_dir_all(&d).map_err(|e| format!("cannot create {}: {e}", d.display()))?;
    #[cfg(unix)]
    {
        // It will hold credentials.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
    }
    Ok(d)
}

/// Delete a named profile and everything in it. Switching away from it first is the caller's job.
pub fn remove(base: &Path, name: &str) -> Result<PathBuf, String> {
    check_exists(base, name)?;
    if name == DEFAULT {
        return Err("the default profile cannot be removed".into());
    }
    let was_active = active(base) == name;
    let d = dir(base, name);
    std::fs::remove_dir_all(&d).map_err(|e| format!("cannot remove {}: {e}", d.display()))?;
    if was_active {
        let _ = set_active(base, DEFAULT);
    }
    Ok(d)
}

/// One line about what a profile holds, for listings. Never shows a credential.
pub fn describe(base: &Path, name: &str) -> String {
    let d = dir(base, name);
    let mut parts = Vec::new();
    if d.join("auth.json").is_file() {
        parts.push("ChatGPT login".to_string());
    }
    let keys = crate::keys::Keys::load(&d.join("keys.json"));
    if !keys.is_empty() {
        parts.push(format!("keys: {}", keys.names().join(", ")));
    }
    if parts.is_empty() {
        "no credentials".into()
    } else {
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(tag: &str) -> PathBuf {
        let b = std::env::temp_dir().join(format!("rig-profile-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&b);
        std::fs::create_dir_all(&b).unwrap();
        b
    }

    #[test]
    fn create_use_list_and_remove() {
        let b = base("cycle");
        assert_eq!(active(&b), DEFAULT);
        create(&b, "work").unwrap();
        assert!(create(&b, "work").is_err());
        set_active(&b, "work").unwrap();
        assert_eq!(active(&b), "work");
        assert_eq!(list(&b), ["default", "work"]);
        assert_eq!(dir(&b, "work"), b.join("profiles").join("work"));
        assert_eq!(dir(&b, DEFAULT), b);
        remove(&b, "work").unwrap();
        assert_eq!(
            active(&b),
            DEFAULT,
            "removing the active profile falls back"
        );
        assert!(!b.join("profile").exists());
        assert!(remove(&b, DEFAULT).is_err());
        std::fs::remove_dir_all(b).ok();
    }

    #[test]
    fn names_cannot_escape_the_profiles_directory() {
        for bad in ["", "../x", "a/b", ".", "-x", &"x".repeat(33)] {
            assert!(validate(bad).is_err(), "{bad:?}");
        }
        assert!(validate("work_2-a").is_ok());
        let b = base("missing");
        assert!(check_exists(&b, "nope").is_err());
        std::fs::write(b.join("profile"), "gone\n").unwrap();
        assert_eq!(active(&b), DEFAULT, "a stale choice falls back");
        std::fs::remove_dir_all(b).ok();
    }
}
