//! Getting a release onto disk and swapping it in. Nothing here is trusted on its own: the caller
//! verifies the signature and digest first, and [`smoke_test`] runs the unpacked binary before it
//! is staged.

use super::release::Version;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Largest archive accepted. A release is a few megabytes; anything bigger is not ours.
pub const MAX_ARCHIVE: u64 = 64 * 1024 * 1024;
/// Largest metadata file accepted (the checksum list and the signature).
pub const MAX_SMALL: u64 = 1024 * 1024;

const USER_AGENT: &str = concat!("rig/", env!("CARGO_PKG_VERSION"));

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .user_agent(USER_AGENT)
        .build()
        .into()
}

/// The response body of `url`, at most `limit` bytes. Errors on any non-success status.
pub fn fetch(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let mut resp = agent()
        .get(url)
        .header("Accept", "*/*")
        .call()
        .map_err(|e| format!("could not fetch {url}: {e}"))?;
    resp.body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| format!("could not read {url}: {e}"))
}

/// Streams `url` into `dest`, refusing anything over `limit` bytes.
pub fn fetch_to_file(url: &str, dest: &Path, limit: u64) -> Result<(), String> {
    let mut resp = agent()
        .get(url)
        .header("Accept", "*/*")
        .call()
        .map_err(|e| format!("could not fetch {url}: {e}"))?;
    let mut reader = resp.body_mut().with_config().limit(limit).reader();
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let copied = std::io::copy(&mut reader, &mut file)
        .map_err(|e| format!("download of {url} failed: {e}"))?;
    file.flush().map_err(|e| e.to_string())?;
    if copied >= limit {
        return Err(format!("{url} is larger than {limit} bytes; refusing it"));
    }
    Ok(())
}

/// Unpacks a `.tar.gz` into `into` with the system `tar`. Only called on archives whose digest has
/// already been checked against the signed list.
pub fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .status()
        .map_err(|e| format!("cannot run tar: {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "tar could not unpack the archive".to_string())
}

/// The unpacked binary must run and report exactly the version it was released as.
pub fn smoke_test(bin: &Path, expect: Version) -> Result<(), String> {
    let out = Command::new(bin)
        .arg("--version")
        .output()
        .map_err(|e| format!("the new binary does not run: {e}"))?;
    let said = String::from_utf8_lossy(&out.stdout);
    match said.split_whitespace().last().and_then(Version::parse) {
        Some(v) if v == expect => Ok(()),
        _ => Err(format!(
            "the new binary reports {:?}, expected {expect}",
            said.trim()
        )),
    }
}

/// Why this binary cannot be replaced in place, if it cannot. Homebrew and development builds are
/// left to their own tools.
pub fn can_swap(exe: &Path) -> Result<(), String> {
    let s = exe.to_string_lossy();
    if s.contains("/Cellar/") || s.contains("/homebrew/") {
        return Err("installed by Homebrew: update with `brew upgrade rig`".into());
    }
    if s.contains("/target/") {
        return Err("this is a development build".into());
    }
    let dir = exe.parent().ok_or("no install folder")?;
    let probe = dir.join(format!(".rig-write-check-{}", std::process::id()));
    std::fs::File::create(&probe).map_err(|_| format!("{} is not writable", dir.display()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Replaces `exe` with `staged`. The old binary is kept as `<name>.prev` for `rig update
/// --rollback`. The replacement is one rename, so `exe` is never missing or half-written.
pub fn swap_in(exe: &Path, staged: &Path) -> std::io::Result<()> {
    let dir = exe
        .parent()
        .ok_or_else(|| std::io::Error::other("no install folder"))?;
    let name = exe
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?
        .to_string_lossy()
        .into_owned();
    let next = dir.join(format!(".{name}.next"));
    std::fs::copy(staged, &next)?;
    set_executable(&next)?;
    let prev = prev_path(exe);
    let _ = std::fs::remove_file(&prev);
    if std::fs::hard_link(exe, &prev).is_err() {
        std::fs::copy(exe, &prev)?;
    }
    std::fs::rename(&next, exe).inspect_err(|_| {
        let _ = std::fs::remove_file(&next);
    })
}

/// Where the previous version is kept.
pub fn prev_path(exe: &Path) -> PathBuf {
    let name = exe.file_name().unwrap_or_default().to_string_lossy();
    exe.with_file_name(format!("{name}.prev"))
}

#[cfg(unix)]
fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn set_executable(_: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rig-stage-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn swapping_keeps_the_old_binary_for_rollback() {
        let d = dir("swap");
        let exe = d.join("rig");
        let staged = d.join("staged-rig");
        std::fs::write(&exe, b"old version").unwrap();
        std::fs::write(&staged, b"new version").unwrap();
        swap_in(&exe, &staged).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new version");
        assert_eq!(std::fs::read(prev_path(&exe)).unwrap(), b"old version");
        assert!(std::fs::metadata(&exe).unwrap().len() > 0);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn homebrew_and_dev_builds_are_not_replaced() {
        assert!(can_swap(Path::new("/opt/homebrew/Cellar/rig/0.1.0/bin/rig")).is_err());
        assert!(can_swap(Path::new("/Users/me/rig/target/release/rig")).is_err());
        let d = dir("writable");
        assert!(can_swap(&d.join("rig")).is_ok());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn a_binary_must_report_the_version_it_was_released_as() {
        let d = dir("smoke");
        let script = d.join("rig");
        std::fs::write(&script, "#!/bin/sh\necho 'rig 0.2.0'\n").unwrap();
        set_executable(&script).unwrap();
        assert!(smoke_test(&script, Version(0, 2, 0)).is_ok());
        assert!(smoke_test(&script, Version(0, 3, 0)).is_err());
        std::fs::remove_dir_all(d).ok();
    }
}
