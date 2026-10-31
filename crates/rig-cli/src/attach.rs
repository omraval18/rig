//! Pictures attached to the message being written: `[Image #1]` tokens in the prompt, the
//! pixels held on the side until the message is sent.

use crate::desktop;
use crate::tui::Tui;
use rig_core::image;
use rig_core::message::Block;
use std::path::PathBuf;

pub struct Attachment {
    pub n: usize,
    pub block: Block,
}

/// `[Image #n]` tokens as `(start, end_exclusive, n)` char indexes.
pub fn image_tokens(chars: &[char]) -> Vec<(usize, usize, usize)> {
    const OPEN: [char; 8] = ['[', 'I', 'm', 'a', 'g', 'e', ' ', '#'];
    let mut out = Vec::new();
    let mut i = 0;
    while i + OPEN.len() < chars.len() {
        if chars[i..i + OPEN.len()] == OPEN {
            let mut j = i + OPEN.len();
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + OPEN.len() && chars.get(j) == Some(&']') {
                let n = chars[i + OPEN.len()..j]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0);
                out.push((i, j + 1, n));
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// A dropped or pasted file path: quoted, backslash-escaped or a `file://` URL.
pub fn dropped_image_path(pasted: &str) -> Option<PathBuf> {
    let t = pasted.trim();
    let t = t.trim_matches(|c| c == '\'' || c == '"');
    let t = t.strip_prefix("file://").unwrap_or(t);
    if t.is_empty() || t.contains('\n') {
        return None;
    }
    let mut path = String::new();
    let mut chars = t.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            path.extend(chars.next());
        } else {
            path.push(c);
        }
    }
    let p = PathBuf::from(path);
    (image::mime_for_path(&p).is_some() && p.is_file()).then_some(p)
}

impl Tui {
    fn attach(&mut self, block: Block, what: &str) {
        let n = self.prompt.next_image;
        self.prompt.next_image += 1;
        let size = if let Block::Image { data, .. } = &block {
            data.len() / 4 * 3 / 1024
        } else {
            0
        };
        self.prompt.attachments.push(Attachment { n, block });
        let before_ok = self.prompt.composer.cursor == 0
            || self
                .prompt
                .composer
                .chars()
                .get(self.prompt.composer.cursor - 1)
                .is_none_or(|c| c.is_whitespace());
        self.prompt.composer.insert_str(&format!(
            "{}[Image #{n}] ",
            if before_ok { "" } else { " " }
        ));
        self.notice(format!("attached {what} ({size} KB)"));
        self.refresh_completion();
    }

    /// Ctrl+V and `/paste`: the image on the clipboard, if there is one.
    pub fn paste_clipboard_image(&mut self) {
        match desktop::read_image() {
            Ok(Some((mime, bytes))) => match image::from_bytes(mime, bytes) {
                Ok(b) => self.attach(b, "image from the clipboard"),
                Err(e) => self.notice(e),
            },
            Ok(None) => self.notice("no image on the clipboard"),
            Err(e) => self.notice(e),
        }
    }

    /// Text pasted into the prompt. An image file path becomes an attachment; an empty paste
    /// (what terminals send for Cmd+V with only an image on the clipboard) tries the clipboard image.
    pub fn paste_text(&mut self, s: &str) {
        if s.is_empty() {
            return self.paste_clipboard_image();
        }
        if let Some(path) = dropped_image_path(s) {
            match image::load(&path) {
                Ok(b) => {
                    let name = path
                        .file_name()
                        .map_or_else(|| "image".into(), |n| n.to_string_lossy().into_owned());
                    return self.attach(b, &name);
                }
                Err(e) => return self.notice(e),
            }
        }
        self.prompt.composer.insert_str(s);
        self.refresh_completion();
    }

    /// The pictures for the tokens still present in `text`, in order. Clears the pending set.
    pub fn take_images(&mut self, text: &str) -> Vec<Block> {
        let chars: Vec<char> = text.chars().collect();
        let blocks = image_tokens(&chars)
            .into_iter()
            .filter_map(|(_, _, n)| {
                self.prompt
                    .attachments
                    .iter()
                    .find(|a| a.n == n)
                    .map(|a| a.block.clone())
            })
            .collect();
        self.prompt.attachments.clear();
        self.prompt.next_image = 1;
        blocks
    }
}

#[cfg(test)]
mod tests;
