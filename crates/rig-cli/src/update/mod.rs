//! Keeps rig current without getting in the way.
//!
//! On start, at most once every 20 hours, rig asks GitHub for the latest release, in a background
//! thread that never draws on the screen. If it is newer, the archive and its signed checksum list
//! are downloaded, the signature and digest are checked, the archive is unpacked, and the binary
//! must run and report the new version. Only then is it staged in `~/.rig/update/`. The next start
//! swaps it in before anything else runs, and keeps the old binary as `rig.prev`.
//!
//! Nothing is run before its signature and digest are checked. When the release key is missing,
//! or the install cannot be replaced in place (Homebrew, a development build, a read-only folder),
//! no download happens at all. `RIG_NO_UPDATE=1` and `rig update --off` turn checks off.

pub mod release;
mod stage;
mod state;
mod verify;

use release::Version;
use state::State;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Minisign public key of the release signing key (the base64 line from `minisign.pub`).
///
/// It is empty until the key is generated and the release workflow is set up. An empty key means
/// rig never downloads anything, which is the safe default.
pub const RELEASE_PUBLIC_KEY: &str = "";

/// How often a background check may run.
pub const CHECK_EVERY_SECS: u64 = 20 * 60 * 60;

/// What `rig update` was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Check now and, if newer, download and stage it.
    Install,
    /// Only report the latest release.
    Check,
    Off,
    On,
    Rollback,
}

/// Whether a check is due. A last-checked time in the future (the clock moved back) counts as due.
fn is_due(last_checked: u64, t: u64) -> bool {
    last_checked > t || t - last_checked >= CHECK_EVERY_SECS
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn update_dir(base: &Path) -> PathBuf {
    base.join("update")
}

fn state_path(base: &Path) -> PathBuf {
    update_dir(base).join("state.json")
}

fn staged_root(base: &Path) -> PathBuf {
    update_dir(base).join("staged")
}

fn staged_bin(base: &Path, v: Version) -> PathBuf {
    staged_root(base).join(v.to_string()).join("rig")
}

/// `RIG_NO_UPDATE` set to anything turns every automatic check off.
pub fn disabled_by_env() -> bool {
    std::env::var_os("RIG_NO_UPDATE").is_some()
}

/// The version waiting for the next start, if it is newer than this binary. Read at startup to
/// show a notice in the footer.
pub fn staged_version(base: &Path) -> Option<Version> {
    let v = State::load(&state_path(base))
        .staged
        .as_deref()
        .and_then(Version::parse)?;
    (v > Version::current()).then_some(v)
}

/// Swaps in a staged update. Returns the path of the new binary when a swap happened, so the
/// caller can start it in place of this process. Failures leave the update staged for next time.
pub fn apply_staged(base: &Path, exe: &Path) -> Option<PathBuf> {
    let path = state_path(base);
    let mut st = State::load(&path);
    let v = st.staged.as_deref().and_then(Version::parse)?;
    if v <= Version::current() || !staged_bin(base, v).is_file() {
        // Already past it, or its file went missing: forget it.
        st.staged = None;
        let _ = st.save(&path);
        let _ = std::fs::remove_dir_all(staged_root(base));
        return None;
    }
    stage::can_swap(exe).ok()?;
    stage::swap_in(exe, &staged_bin(base, v)).ok()?;
    st.staged = None;
    let _ = st.save(&path);
    let _ = std::fs::remove_dir_all(staged_root(base));
    Some(exe.to_path_buf())
}

/// Starts a check in a background thread if one is due. Returns at once. The thread prints
/// nothing, because it would draw over the UI.
pub fn check_in_background(base: &Path, exe: &Path) {
    if disabled_by_env() || RELEASE_PUBLIC_KEY.trim().is_empty() {
        return;
    }
    let st = State::load(&state_path(base));
    if st.disabled || !is_due(st.last_checked, now()) || stage::can_swap(exe).is_err() {
        return;
    }
    let base = base.to_path_buf();
    let _ = std::thread::Builder::new()
        .name("rig-update".into())
        .spawn(move || {
            let _ = run(&base);
        });
}

/// Asks GitHub for the latest release, and stages it if it is newer. `Ok(Some(v))` means `v` is
/// staged; `Ok(None)` means this binary is already the latest.
fn run(base: &Path) -> Result<Option<Version>, String> {
    let path = state_path(base);
    let mut st = State::load(&path);
    st.last_checked = now();
    let body = stage::fetch(release::LATEST_API, stage::MAX_SMALL)?;
    let latest = release::latest_from_json(&String::from_utf8_lossy(&body))
        .ok_or("GitHub returned no usable release")?;
    st.latest = Some(latest.to_string());
    st.save(&path).map_err(|e| e.to_string())?;
    if latest <= Version::current() {
        return Ok(None);
    }

    let target = release::target().ok_or("no rig build is published for this platform yet")?;
    let work = update_dir(base).join("work");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let result = download_and_check(base, latest, target, &work);
    let _ = std::fs::remove_dir_all(&work);
    result?;

    st.staged = Some(latest.to_string());
    st.save(&path).map_err(|e| e.to_string())?;
    Ok(Some(latest))
}

fn download_and_check(base: &Path, v: Version, target: &str, work: &Path) -> Result<(), String> {
    let sums = stage::fetch(&release::asset_url(v, "SHA256SUMS"), stage::MAX_SMALL)?;
    let sig = stage::fetch(
        &release::asset_url(v, "SHA256SUMS.minisig"),
        stage::MAX_SMALL,
    )?;
    verify::check_signature(RELEASE_PUBLIC_KEY, &sums, &String::from_utf8_lossy(&sig))?;

    let archive_name = release::archive_name(v, target);
    let sums_text = String::from_utf8_lossy(&sums);
    let digest = verify::expected_digest(&sums_text, &archive_name)
        .ok_or("the signed checksum list has no entry for this platform")?;
    let archive = work.join(&archive_name);
    stage::fetch_to_file(
        &release::asset_url(v, &archive_name),
        &archive,
        stage::MAX_ARCHIVE,
    )?;
    let actual = verify::file_sha256(&archive).map_err(|e| e.to_string())?;
    if actual != digest {
        return Err("the download does not match the signed checksum; discarded".into());
    }

    stage::unpack(&archive, work)?;
    let bin = work.join(release::archive_dir(v, target)).join("rig");
    stage::smoke_test(&bin, v)?;

    // Replace any older staged release wholesale.
    let _ = std::fs::remove_dir_all(staged_root(base));
    let dest = staged_bin(base, v);
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&bin, &dest)
        .or_else(|_| std::fs::copy(&bin, &dest).map(drop))
        .map_err(|e| e.to_string())
}

/// `rig update` from the command line. Returns the process exit code.
pub fn command(base: &Path, exe: &Path, action: Action) -> i32 {
    let path = state_path(base);
    match action {
        Action::On | Action::Off => {
            let mut st = State::load(&path);
            st.disabled = action == Action::Off;
            match st.save(&path) {
                Ok(()) if st.disabled => {
                    println!("Automatic update checks are off. `rig update` still works.");
                    0
                }
                Ok(()) => {
                    println!("Automatic update checks are on.");
                    0
                }
                Err(e) => fail(&e.to_string()),
            }
        }
        Action::Rollback => {
            if let Err(e) = stage::can_swap(exe) {
                return fail(&e);
            }
            let prev = stage::prev_path(exe);
            if !prev.is_file() {
                return fail("there is no previous version to go back to");
            }
            match std::fs::rename(&prev, exe) {
                Ok(()) => {
                    println!("Went back to the previous rig. Restart rig to use it.");
                    0
                }
                Err(e) => fail(&e.to_string()),
            }
        }
        Action::Check => match check_only(base) {
            Ok(Some(v)) => {
                println!(
                    "rig {v} is available (you have {}). Run `rig update` to download it.",
                    Version::current()
                );
                0
            }
            Ok(None) => {
                println!("rig {} is the latest release.", Version::current());
                0
            }
            Err(e) => fail(&e),
        },
        Action::Install => {
            if RELEASE_PUBLIC_KEY.trim().is_empty() {
                return fail(
                    "this build has no release key, so it cannot verify downloads. Build a release from source instead (see install.sh).",
                );
            }
            if let Err(e) = stage::can_swap(exe) {
                return fail(&e);
            }
            match run(base) {
                Ok(Some(v)) => {
                    println!("rig {v} is downloaded and verified. Restart rig to start using it.");
                    0
                }
                Ok(None) => {
                    println!("rig {} is the latest release.", Version::current());
                    0
                }
                Err(e) => fail(&e),
            }
        }
    }
}

/// Reports the latest release without downloading it.
fn check_only(base: &Path) -> Result<Option<Version>, String> {
    let body = stage::fetch(release::LATEST_API, stage::MAX_SMALL)?;
    let latest = release::latest_from_json(&String::from_utf8_lossy(&body))
        .ok_or("GitHub returned no usable release")?;
    let path = state_path(base);
    let mut st = State::load(&path);
    st.last_checked = now();
    st.latest = Some(latest.to_string());
    let _ = st.save(&path);
    Ok((latest > Version::current()).then_some(latest))
}

fn fail(message: &str) -> i32 {
    eprintln!("error: {message}");
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_without_a_release_key_never_downloads() {
        // The shipped default is the safe one: no key, no network, no staging.
        assert!(RELEASE_PUBLIC_KEY.is_empty());
    }

    #[test]
    fn checks_are_spaced_out_and_survive_a_clock_that_went_backwards() {
        assert!(!is_due(1_000, 1_000 + CHECK_EVERY_SECS - 1));
        assert!(is_due(1_000, 1_000 + CHECK_EVERY_SECS));
        assert!(is_due(5_000, 1_000));
    }
}
