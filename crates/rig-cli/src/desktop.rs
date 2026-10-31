//! The desktop around the terminal: the clipboard and the default browser.

use rig_core::image::sniff;
use std::io::Write;
use std::process::{Command, Stdio};

/// The image on the clipboard as `(mime, bytes)`. `Ok(None)` means the clipboard holds something else.
pub fn read_image() -> Result<Option<(&'static str, Vec<u8>)>, String> {
    if cfg!(target_os = "macos") {
        return Ok(macos());
    }
    let tools: [(&str, &[&str]); 2] = [
        ("wl-paste", &["--type", "image/png"]),
        (
            "xclip",
            &["-selection", "clipboard", "-t", "image/png", "-o"],
        ),
    ];
    let mut found = false;
    for (prog, args) in tools {
        let Ok(out) = Command::new(prog)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        else {
            continue;
        };
        found = true;
        if let Some(mime) = sniff(&out.stdout) {
            return Ok(Some((mime, out.stdout)));
        }
    }
    if found {
        Ok(None)
    } else {
        Err("no clipboard tool found (install wl-clipboard or xclip to paste images)".into())
    }
}

/// AppleScript can write the pasteboard's PNG or JPEG data to a file; nothing else ships with macOS.
fn macos() -> Option<(&'static str, Vec<u8>)> {
    for class in ["PNGf", "JPEG"] {
        let path = std::env::temp_dir().join(format!("rig-clip-{}-{class}", std::process::id()));
        let script = [
            "try".to_string(),
            format!("set d to (the clipboard as «class {class}»)"),
            format!(
                "set f to open for access POSIX file \"{}\" with write permission",
                path.display()
            ),
            "set eof f to 0".to_string(),
            "write d to f".to_string(),
            "close access f".to_string(),
            "return \"ok\"".to_string(),
            "on error".to_string(),
            "return \"none\"".to_string(),
            "end try".to_string(),
        ];
        let mut cmd = Command::new("osascript");
        for line in &script {
            cmd.arg("-e").arg(line);
        }
        let ok = cmd
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "ok");
        let bytes = if ok { std::fs::read(&path).ok() } else { None };
        let _ = std::fs::remove_file(&path);
        if let Some(b) = bytes
            && let Some(mime) = sniff(&b)
        {
            return Some((mime, b));
        }
    }
    None
}

/// Put `text` on the clipboard. Returns the tool that did it.
pub fn copy_text(text: &str) -> Result<&'static str, String> {
    for (prog, args) in [
        ("pbcopy", &[][..]),
        ("wl-copy", &[][..]),
        ("xclip", &["-selection", "clipboard"][..]),
        ("xsel", &["--clipboard", "--input"][..]),
    ] {
        let Ok(mut child) = Command::new(prog)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        if child.wait().is_ok_and(|s| s.success()) {
            return Ok(prog);
        }
    }
    Err("no clipboard tool found (tried pbcopy, wl-copy, xclip, xsel)".into())
}

/// Open `url` in the default browser. Skipped when `RIG_NO_BROWSER` is set (tests, headless boxes).
pub fn open_url(url: &str) {
    if std::env::var_os("RIG_NO_BROWSER").is_some() {
        return;
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = Command::new(opener)
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}
