//! Filesystem helpers shared by tools and `@` expansion.

use std::path::Path;

/// `(name, is_dir)` for every entry of `dir`: folders first, then by name.
pub fn list_dir(dir: &Path) -> std::io::Result<Vec<(String, bool)>> {
    let mut entries: Vec<(String, bool)> = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                e.path().is_dir(),
            )
        })
        .collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(entries)
}

/// Replace `path` with `bytes` atomically (temp file, then rename), so a crash never leaves half a
/// file. Skips the write entirely when the file already holds exactly these bytes, which keeps
/// settings toggles and repeated saves from wearing the disk. `mode` applies to the new file
/// (`0o600` for credentials). Returns whether anything was written.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<bool> {
    let same_len = std::fs::metadata(path).is_ok_and(|m| m.len() == bytes.len() as u64);
    if same_len && std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp{}", std::process::id()));
    let tmp = std::path::PathBuf::from(tmp);
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, mode);
        #[cfg(not(unix))]
        let _ = mode;
        opts.open(&tmp)?.write_all(bytes)?;
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_skips_identical_content() {
        let dir = std::env::temp_dir().join(format!("rig-fsutil-{}", std::process::id()));
        let f = dir.join("sub").join("state.json");
        assert!(write_atomic(&f, b"{}", 0o600).unwrap());
        assert!(
            !write_atomic(&f, b"{}", 0o600).unwrap(),
            "same bytes: no write"
        );
        assert!(write_atomic(&f, b"{ }", 0o600).unwrap());
        assert_eq!(std::fs::read(&f).unwrap(), b"{ }");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&f).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
