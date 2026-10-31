//! Pictures the user attaches: loading, shrinking and encoding.

use crate::message::Block;
use std::path::Path;

/// Larger than this is downscaled first (providers cap request sizes and charge by pixels).
const SHRINK_ABOVE: usize = 2 * 1024 * 1024;
/// Never send more than this, even after shrinking.
const MAX_BYTES: usize = 15 * 1024 * 1024;
const MAX_EDGE: &str = "2000";

pub fn mime_for_path(p: &Path) -> Option<&'static str> {
    match p.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// What the bytes really are, whatever the name says.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [0xff, 0xd8, 0xff, ..] => Some("image/jpeg"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some("image/webp"),
        _ => None,
    }
}

pub fn from_bytes(mime: &str, bytes: Vec<u8>) -> Result<Block, String> {
    let bytes = if bytes.len() > SHRINK_ABOVE {
        shrink(bytes, mime)
    } else {
        bytes
    };
    if bytes.len() > MAX_BYTES {
        return Err(format!(
            "image is {} MB; the limit is {} MB",
            bytes.len() / (1024 * 1024),
            MAX_BYTES / (1024 * 1024)
        ));
    }
    Ok(Block::Image {
        mime: mime.to_string(),
        data: base64(&bytes),
    })
}

pub fn load(path: &Path) -> Result<Block, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mime = sniff(&bytes)
        .or_else(|| mime_for_path(path))
        .ok_or_else(|| format!("{} is not a PNG, JPEG, GIF or WebP image", path.display()))?;
    from_bytes(mime, bytes)
}

/// Downscale a large image with the system tool where there is one (macOS `sips`); otherwise keep it.
fn shrink(bytes: Vec<u8>, mime: &str) -> Vec<u8> {
    if !cfg!(target_os = "macos") || mime == "image/gif" {
        return bytes;
    }
    let ext = if mime == "image/png" {
        "png"
    } else if mime == "image/webp" {
        "webp"
    } else {
        "jpg"
    };
    let dir = std::env::temp_dir().join(format!("rig-img-{}-{}", std::process::id(), bytes.len()));
    if std::fs::create_dir_all(&dir).is_err() {
        return bytes;
    }
    let (src, dst) = (
        dir.join(format!("in.{ext}")),
        dir.join(format!("out.{ext}")),
    );
    let shrunk = std::fs::write(&src, &bytes)
        .ok()
        .and_then(|_| {
            std::process::Command::new("sips")
                .args(["-Z", MAX_EDGE])
                .arg(&src)
                .arg("--out")
                .arg(&dst)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .ok()
        })
        .filter(std::process::ExitStatus::success)
        .and_then(|_| std::fs::read(&dst).ok())
        .filter(|b| !b.is_empty() && b.len() < bytes.len());
    let _ = std::fs::remove_dir_all(&dir);
    shrunk.unwrap_or(bytes)
}

pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests;
