//! Which release is newer, and which archive belongs to this machine.

use std::fmt;

pub const LATEST_API: &str = "https://api.github.com/repos/omraval18/rig/releases/latest";
/// Assets are fetched from here only; a release that points anywhere else is not followed.
pub const DOWNLOAD_BASE: &str = "https://github.com/omraval18/rig/releases/download";

/// `MAJOR.MINOR.PATCH`. Ordering compares the numbers, which is what semver precedence means for
/// releases without pre-release tags (pre-releases are never offered).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// Strict: exactly three numbers, with an optional leading `v`. `0.2`, `1.0.0-rc1` and
    /// `1.0.0.0` are rejected.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let mut parts = s.split('.');
        let v = Version(num(parts.next())?, num(parts.next())?, num(parts.next())?);
        parts.next().is_none().then_some(v)
    }

    /// The version this binary was built as.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
    }
}

fn num(part: Option<&str>) -> Option<u64> {
    let p = part?;
    if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    p.parse().ok()
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// The release named by GitHub's `releases/latest` response, if it has a valid tag.
pub fn latest_from_json(body: &str) -> Option<Version> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    Version::parse(v["tag_name"].as_str()?)
}

/// The build published for this machine, or `None` when no build exists for it.
pub fn target() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        _ => return None,
    })
}

/// File name of the release archive, e.g. `rig-0.2.0-aarch64-apple-darwin.tar.gz`.
pub fn archive_name(v: Version, target: &str) -> String {
    format!("rig-{v}-{target}.tar.gz")
}

/// The folder the archive unpacks into; the binary is inside it.
pub fn archive_dir(v: Version, target: &str) -> String {
    format!("rig-{v}-{target}")
}

pub fn asset_url(v: Version, file: &str) -> String {
    format!("{DOWNLOAD_BASE}/v{v}/{file}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_strictly_and_order_numerically() {
        assert_eq!(Version::parse("v0.2.0"), Some(Version(0, 2, 0)));
        assert_eq!(Version::parse("1.10.0"), Some(Version(1, 10, 0)));
        for bad in ["0.2", "1.0.0-rc1", "1.0.0.0", "", "a.b.c", "1..0"] {
            assert!(Version::parse(bad).is_none(), "{bad:?}");
        }
        // Numeric, not textual: 0.10.0 is newer than 0.9.0.
        assert!(Version::parse("0.10.0") > Version::parse("0.9.0"));
    }

    #[test]
    fn latest_release_json_gives_its_tag() {
        let body = r#"{"tag_name":"v0.3.1","name":"rig 0.3.1","prerelease":false}"#;
        assert_eq!(latest_from_json(body), Some(Version(0, 3, 1)));
        assert_eq!(latest_from_json(r#"{"tag_name":"nightly"}"#), None);
        assert_eq!(latest_from_json("not json"), None);
    }

    #[test]
    fn archive_and_url_names_match_the_release_workflow() {
        let v = Version(0, 2, 0);
        assert_eq!(
            archive_name(v, "aarch64-apple-darwin"),
            "rig-0.2.0-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            archive_dir(v, "x86_64-unknown-linux-gnu"),
            "rig-0.2.0-x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            asset_url(v, "SHA256SUMS"),
            "https://github.com/omraval18/rig/releases/download/v0.2.0/SHA256SUMS"
        );
    }
}
