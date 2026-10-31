//! Parsing the patch text into per-file operations.

#[derive(Debug, PartialEq)]
pub(super) enum Op {
    Add {
        path: String,
        content: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        move_to: Option<String>,
        hunks: Vec<Hunk>,
    },
}

#[derive(Debug, PartialEq, Default)]
pub(super) struct Hunk {
    pub(super) anchor: Option<String>,
    /// (' ' | '-' | '+', text)
    pub(super) lines: Vec<(char, String)>,
    pub(super) at_eof: bool,
}

pub(super) fn parse(patch: &str) -> Result<Vec<Op>, String> {
    let lines: Vec<&str> = patch
        .lines()
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut i = lines
        .iter()
        .position(|l| l.trim() == "*** Begin Patch")
        .ok_or("missing '*** Begin Patch'")?
        + 1;
    let mut ops = Vec::new();
    while i < lines.len() {
        let line = lines[i];
        if line.trim() == "*** End Patch" {
            return if ops.is_empty() {
                Err("patch has no operations".into())
            } else {
                Ok(ops)
            };
        }
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            i += 1;
            let mut content = String::new();
            while i < lines.len() && !lines[i].starts_with("*** ") {
                let l = lines[i].strip_prefix('+').ok_or_else(|| {
                    format!(
                        "line in added file {path} must start with '+': {:?}",
                        lines[i]
                    )
                })?;
                content.push_str(l);
                content.push('\n');
                i += 1;
            }
            ops.push(Op::Add {
                path: path.trim().into(),
                content,
            });
        } else if let Some(path) = line.strip_prefix("*** Delete File: ") {
            ops.push(Op::Delete {
                path: path.trim().into(),
            });
            i += 1;
        } else if let Some(path) = line.strip_prefix("*** Update File: ") {
            i += 1;
            let mut move_to = None;
            if let Some(dest) = lines.get(i).and_then(|l| l.strip_prefix("*** Move to: ")) {
                move_to = Some(dest.trim().to_string());
                i += 1;
            }
            let mut hunks: Vec<Hunk> = Vec::new();
            while i < lines.len() {
                let l = lines[i];
                if l.trim() == "*** End of File" {
                    if let Some(h) = hunks.last_mut() {
                        h.at_eof = true;
                    }
                    i += 1;
                    continue;
                }
                if l.starts_with("*** ") {
                    break;
                }
                if let Some(rest) = l.strip_prefix("@@") {
                    let anchor = rest.trim();
                    hunks.push(Hunk {
                        anchor: (!anchor.is_empty()).then(|| anchor.to_string()),
                        ..Hunk::default()
                    });
                } else {
                    let (tag, text) = match l.chars().next() {
                        Some(c @ (' ' | '-' | '+')) => (c, &l[1..]),
                        None => (' ', ""),
                        Some(_) => {
                            return Err(format!(
                                "unexpected line in {path}: {l:?} (hunk lines start with ' ', '-' or '+')"
                            ));
                        }
                    };
                    let line = (tag, text.to_string());
                    match hunks.last_mut() {
                        Some(hunk) => hunk.lines.push(line),
                        None => hunks.push(Hunk {
                            lines: vec![line],
                            ..Hunk::default()
                        }),
                    }
                }
                i += 1;
            }
            if hunks.is_empty() && move_to.is_none() {
                return Err(format!("no changes given for {path}"));
            }
            ops.push(Op::Update {
                path: path.trim().into(),
                move_to,
                hunks,
            });
        } else if line.trim().is_empty() {
            i += 1;
        } else {
            return Err(format!("unexpected line: {line:?}"));
        }
    }
    Err("missing '*** End Patch'".into())
}
