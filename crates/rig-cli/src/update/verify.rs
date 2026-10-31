//! Checks a download before anything runs it. The release publishes a `SHA256SUMS` file and a
//! minisign signature over it. The archive must be listed there with the right digest, and the
//! list must carry a valid signature from the release key compiled into this binary.

use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// The digest listed for `archive`. Accepts both `sha256sum` line forms: `<hex>  name` and the
/// binary-mode `<hex> *name`.
pub fn expected_digest(sums: &str, archive: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (digest, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        let well_formed = digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit());
        (name == archive && well_formed).then(|| digest.to_ascii_lowercase())
    })
}

/// Fails unless `data` was signed by `public_key`, a minisign public key (base64, or a whole
/// `minisign.pub` file).
/// An empty key means this build has no release key, so nothing is trusted.
pub fn check_signature(public_key: &str, data: &[u8], signature: &str) -> Result<(), String> {
    if public_key.trim().is_empty() {
        return Err("this build has no release key, so it cannot verify updates".into());
    }
    // Accept the bare base64 line or a whole `minisign.pub` file (which has a comment line first).
    let line = public_key
        .lines()
        .rfind(|l| !l.trim().is_empty() && !l.starts_with("untrusted comment"))
        .unwrap_or_default()
        .trim();
    let key = PublicKey::from_base64(line).map_err(|e| format!("bad release key: {e}"))?;
    let sig = Signature::decode(signature).map_err(|e| format!("bad signature file: {e}"))?;
    key.verify(data, &sig, false)
        .map_err(|e| format!("signature check failed: {e}"))
}

/// SHA-256 of a file, streamed so a large archive is never read into memory at once.
pub fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = include_str!("testdata/test.pub");
    const SUMS: &[u8] = include_bytes!("testdata/SHA256SUMS");
    const SIG: &str = include_str!("testdata/SHA256SUMS.minisig");

    #[test]
    fn the_listed_digest_is_found_in_both_sha256sum_forms() {
        let sums = std::str::from_utf8(SUMS).unwrap_or_default();
        let a = "a".repeat(64);
        assert_eq!(
            expected_digest(sums, "rig-0.2.0-aarch64-apple-darwin.tar.gz"),
            Some(a)
        );
        assert_eq!(
            expected_digest(sums, "rig-0.2.0-x86_64-unknown-linux-gnu.tar.gz"),
            Some("b".repeat(64))
        );
        assert_eq!(expected_digest(sums, "rig-0.9.9-other.tar.gz"), None);
    }

    #[test]
    fn a_malformed_digest_is_never_accepted() {
        let sums = format!("{} x.tar.gz\n", "z".repeat(64));
        assert_eq!(expected_digest(&sums, "x.tar.gz"), None);
        assert_eq!(expected_digest("abc  x.tar.gz", "x.tar.gz"), None);
    }

    #[test]
    fn a_checksum_list_signed_by_the_release_key_passes() {
        assert_eq!(check_signature(TEST_KEY, SUMS, SIG), Ok(()));
    }

    #[test]
    fn a_tampered_checksum_list_fails() {
        let mut tampered = SUMS.to_vec();
        tampered[0] = b'c';
        assert!(check_signature(TEST_KEY, &tampered, SIG).is_err());
    }

    #[test]
    fn a_signature_from_another_key_fails() {
        // A different real key: its signature check must fail for the right reason.
        let other = "RWRQEGDVzsC0PJwX8873OHHp14PNr0HTjAoRI1dLvulcQyNB7wRuYpnC";
        assert!(check_signature(other, SUMS, SIG).is_err());
    }

    #[test]
    fn without_a_release_key_nothing_is_trusted() {
        assert!(check_signature("", SUMS, SIG).is_err());
        assert!(check_signature("   ", SUMS, SIG).is_err());
    }
}
