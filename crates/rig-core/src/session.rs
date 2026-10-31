//! Append-only JSONL session files, one per conversation, grouped by project.
//!
//! The file is a log: messages are appended as they happen, and a compaction is
//! recorded as a `checkpoint` entry that replaces a range of earlier messages when the
//! log is loaded. Nothing already written is ever rewritten.
//!
//! A new session stays in memory until it holds something worth keeping (a message from the
//! user or the model), so opening rig and quitting without typing never touches the disk.

use crate::message::{Block, Message, Role};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// One line of the log. Messages are borrowed when writing, so appending never copies them
/// (images included).
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Record<'a> {
    Meta {
        id: String,
        cwd: String,
        model: String,
        created: u64,
    },
    Message {
        message: Cow<'a, Message>,
    },
    /// Messages `1..split` of the history at this point became `message`.
    Checkpoint {
        split: usize,
        message: Cow<'a, Message>,
    },
    Title {
        title: String,
    },
    /// What this session was using: restored when it is resumed.
    Settings {
        model: String,
        #[serde(default)]
        effort: Option<String>,
        autonomy: String,
    },
    /// The history was cut back to `len` messages (a turn that failed before the model answered).
    Truncate {
        len: usize,
    },
}

/// The model, reasoning effort and permission level a session was using.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionSettings {
    /// `provider/model`.
    pub model: Option<String>,
    /// `low`..`xhigh` or `none`; `None` when the session never recorded one.
    pub effort: Option<String>,
    pub autonomy: Option<String>,
}

pub struct Session {
    pub id: String,
    pub path: PathBuf,
    /// `None` until the session is worth keeping; records wait in `pending` meanwhile.
    file: Option<File>,
    pending: Vec<u8>,
    /// What the log last recorded, so re-recording the same settings writes nothing.
    settings: SessionSettings,
}

/// One row in the resume picker.
#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub id: String,
    pub path: PathBuf,
    pub title: String,
    pub messages: usize,
    pub modified: SystemTime,
}

fn project_dir(root: &Path, cwd: &Path) -> PathBuf {
    let slug: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    root.join("sessions").join(slug)
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Where compacted turns are archived for a session file.
pub fn archive_path(session_file: &Path) -> PathBuf {
    session_file.with_extension("archive.jsonl")
}

impl Session {
    /// Start a new session under `root` (usually `~/.rig`). Nothing is written until the first
    /// message that is not harness context (see [`Session::append`]).
    pub fn create(root: &Path, cwd: &Path, model: &str) -> io::Result<Self> {
        let id = format!("{:x}-{:x}", now_nanos() / 1_000, std::process::id());
        let path = project_dir(root, cwd).join(format!("{id}.jsonl"));
        let mut s = Self {
            id: id.clone(),
            path,
            file: None,
            pending: Vec::new(),
            settings: SessionSettings::default(),
        };
        s.write(&Record::Meta {
            id,
            cwd: cwd.to_string_lossy().into_owned(),
            model: model.to_string(),
            created: (now_nanos() / 1_000_000_000) as u64,
        })?;
        Ok(s)
    }

    /// Reopen an existing session file for appending.
    pub fn reopen(path: &Path) -> io::Result<Self> {
        let id = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let file = OpenOptions::new().append(true).open(path)?;
        Ok(Self {
            id,
            path: path.to_path_buf(),
            file: Some(file),
            pending: Vec::new(),
            settings: Self::settings(path),
        })
    }

    /// Whether the session has a file on disk yet.
    pub fn is_saved(&self) -> bool {
        self.file.is_some()
    }

    /// A new session that starts from `messages` (used by `/fork`). Copies the archive so
    /// handles in a compaction checkpoint keep working.
    pub fn fork(
        root: &Path,
        cwd: &Path,
        model: &str,
        from: &Path,
        messages: &[Message],
    ) -> io::Result<Self> {
        let mut s = Self::create(root, cwd, model)?;
        for m in messages {
            s.append(m)?;
        }
        s.persist()?;
        let src = archive_path(from);
        if src.is_file() {
            fs::copy(src, archive_path(&s.path))?;
        }
        Ok(s)
    }

    pub fn archive(&self) -> PathBuf {
        archive_path(&self.path)
    }

    /// Append a message. The first one that is not harness context puts a new session on disk.
    pub fn append(&mut self, message: &Message) -> io::Result<()> {
        self.write(&Record::Message {
            message: Cow::Borrowed(message),
        })?;
        if !is_context_only(message) {
            self.persist()?;
        }
        Ok(())
    }

    pub fn append_checkpoint(&mut self, split: usize, message: &Message) -> io::Result<()> {
        self.write(&Record::Checkpoint {
            split,
            message: Cow::Borrowed(message),
        })
    }

    /// Record what the session is using now, so resuming it later brings the same model and permissions back.
    pub fn append_settings(
        &mut self,
        model: &str,
        effort: Option<&str>,
        autonomy: &str,
    ) -> io::Result<()> {
        let now = SessionSettings {
            model: Some(model.to_string()),
            effort: effort.map(String::from),
            autonomy: Some(autonomy.to_string()),
        };
        if now == self.settings {
            return Ok(());
        }
        self.write(&Record::Settings {
            model: model.to_string(),
            effort: effort.map(String::from),
            autonomy: autonomy.to_string(),
        })?;
        self.settings = now;
        Ok(())
    }

    /// The last settings a session recorded; older sessions fall back to the model they were created with.
    pub fn settings(path: &Path) -> SessionSettings {
        let mut out = SessionSettings::default();
        let Ok(f) = File::open(path) else { return out };
        for line in BufReader::new(f).lines().map_while(Result::ok) {
            // Only the small meta and settings lines are parsed; messages are skipped unread.
            if !matches!(record_kind(&line), Some("meta" | "settings") | None) {
                continue;
            }
            match serde_json::from_str(&line) {
                Ok(Record::Meta { model, .. }) if out.model.is_none() => out.model = Some(model),
                Ok(Record::Settings {
                    model,
                    effort,
                    autonomy,
                }) => {
                    out = SessionSettings {
                        model: Some(model),
                        effort,
                        autonomy: Some(autonomy),
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub fn append_truncate(&mut self, len: usize) -> io::Result<()> {
        self.write(&Record::Truncate { len })
    }

    pub fn set_title(&mut self, title: &str) -> io::Result<()> {
        self.write(&Record::Title {
            title: title.to_string(),
        })?;
        self.persist()
    }

    /// Put the session on disk now, with everything recorded so far.
    pub fn persist(&mut self) -> io::Result<()> {
        if self.file.is_some() {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(&self.pending)?;
        self.pending = Vec::new();
        self.file = Some(file);
        Ok(())
    }

    /// One `write` per record, so a line is never split between two writers.
    fn write(&mut self, r: &Record) -> io::Result<()> {
        match self.file.as_mut() {
            Some(f) => {
                let mut line = serde_json::to_vec(r).map_err(io::Error::other)?;
                line.push(b'\n');
                f.write_all(&line)
            }
            None => {
                serde_json::to_writer(&mut self.pending, r).map_err(io::Error::other)?;
                self.pending.push(b'\n');
                Ok(())
            }
        }
    }

    /// The conversation as the model last saw it: messages with checkpoints applied.
    pub fn load(path: &Path) -> io::Result<Vec<Message>> {
        let mut out: Vec<Message> = Vec::new();
        for line in BufReader::new(File::open(path)?).lines() {
            match serde_json::from_str(&line?) {
                Ok(Record::Message { message }) => out.push(message.into_owned()),
                Ok(Record::Truncate { len }) => out.truncate(len),
                Ok(Record::Checkpoint { split, message }) if split >= 1 && split <= out.len() => {
                    out.splice(1..split, std::iter::once(message.into_owned()));
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// Latest title if one was set, else the first line the user typed; and the message count.
    /// Only titles and messages up to the first prompt are parsed; the rest are just counted.
    fn summarize(path: &Path) -> (String, usize) {
        let (mut title, mut first_prompt, mut count) = (None, None, 0);
        let Ok(f) = File::open(path) else {
            return (String::new(), 0);
        };
        for line in BufReader::new(f).lines().map_while(Result::ok) {
            let kind = record_kind(&line);
            match kind {
                Some("message") => count += 1,
                Some("title") | None => {}
                Some(_) => continue,
            }
            if kind == Some("message") && first_prompt.is_some() {
                continue;
            }
            match serde_json::from_str(&line) {
                Ok(Record::Title { title: t }) => title = Some(t),
                Ok(Record::Message { message }) => {
                    if kind.is_none() {
                        count += 1;
                    }
                    if first_prompt.is_none() && message.role == Role::User {
                        let text = message.blocks.iter().find_map(|b| match b {
                            Block::Text { text } if !crate::prompt::is_context(text) => {
                                Some(text.as_str())
                            }
                            _ => None,
                        });
                        first_prompt =
                            text.map(|t| t.lines().next().unwrap_or("").trim().to_string());
                    }
                }
                _ => {}
            }
        }
        (
            title
                .or(first_prompt)
                .unwrap_or_else(|| "(empty session)".into()),
            count,
        )
    }

    /// Session files for this project with their modification times, newest first. Cheap: no file
    /// is opened.
    fn files(root: &Path, cwd: &Path) -> Vec<(PathBuf, SystemTime)> {
        let Ok(rd) = fs::read_dir(project_dir(root, cwd)) else {
            return Vec::new();
        };
        let mut out: Vec<(PathBuf, SystemTime)> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|x| x == "jsonl")
                    && !p.to_string_lossy().ends_with(".archive.jsonl")
            })
            .filter_map(|p| {
                let modified = fs::metadata(&p).and_then(|m| m.modified()).ok()?;
                Some((p, modified))
            })
            .collect();
        out.sort_by_key(|(_, m)| std::cmp::Reverse(*m));
        out
    }

    fn info(path: PathBuf, modified: SystemTime) -> Option<SessionInfo> {
        let (title, messages) = Self::summarize(&path);
        let id = path.file_stem()?.to_string_lossy().into_owned();
        (messages > 1).then_some(SessionInfo {
            id,
            path,
            title,
            messages,
            modified,
        })
    }

    /// Sessions for this project, newest first.
    pub fn list(root: &Path, cwd: &Path) -> Vec<SessionInfo> {
        Self::files(root, cwd)
            .into_iter()
            .filter_map(|(p, m)| Self::info(p, m))
            .collect()
    }

    /// Most recently modified session for this project. Stops at the first one with a conversation.
    pub fn latest(root: &Path, cwd: &Path) -> Option<PathBuf> {
        Self::files(root, cwd)
            .into_iter()
            .find_map(|(p, m)| Self::info(p, m))
            .map(|s| s.path)
    }

    /// Resolve a session id (or unique prefix) for this project. Only files whose name matches are read.
    pub fn find(root: &Path, cwd: &Path, id: &str) -> Option<PathBuf> {
        let mut hits = Self::files(root, cwd)
            .into_iter()
            .filter(|(p, _)| {
                p.file_stem()
                    .is_some_and(|s| s.to_string_lossy().starts_with(id))
            })
            .filter_map(|(p, m)| Self::info(p, m));
        let first = hits.next()?;
        hits.next().is_none().then_some(first.path)
    }
}

/// The `kind` of a log line without parsing it. Records are written with `kind` first; `None` means
/// the line does not look like that, and the caller should parse it to find out.
fn record_kind(line: &str) -> Option<&str> {
    let rest = line.strip_prefix(r#"{"kind":""#)?;
    rest.split_once('"').map(|(k, _)| k)
}

/// A message made only of `<rig-context>` blocks: what a session starts with, not worth a file.
fn is_context_only(m: &Message) -> bool {
    m.role == Role::User
        && m.blocks
            .iter()
            .all(|b| matches!(b, Block::Text { text } if crate::prompt::is_context(text)))
}

/// "3m ago" style age for pickers.
pub fn age(t: SystemTime) -> String {
    let secs = SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests;
