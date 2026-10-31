//! Fuzzy matching shared by `/` commands, `@` files, `$` skills and every picker.
//!
//! Ranking follows fx: an exact name beats a name prefix, which beats a whole
//! word, which beats scattered letters. Matched positions are returned so the
//! UI can highlight the letters that earned the rank.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub score: i32,
    /// Char indexes into the candidate that matched the query.
    pub positions: Vec<usize>,
}

fn is_boundary(prev: Option<char>) -> bool {
    prev.is_none_or(|p| !p.is_alphanumeric())
}

/// Subsequence match of `query` in `candidate`, case-insensitive. `None` if some letter is missing.
pub fn subsequence(query: &str, candidate: &str) -> Option<Match> {
    let q: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(Match {
            score: 0,
            positions: vec![],
        });
    }
    let chars: Vec<char> = candidate.chars().collect();
    let lower: Vec<char> = chars.iter().flat_map(|c| c.to_lowercase()).collect();
    // Lowercasing can change length for exotic scripts; fall back to a plain scan then.
    if lower.len() != chars.len() {
        return None;
    }
    let (mut qi, mut score, mut last) = (0usize, 0i32, None::<usize>);
    let mut positions = Vec::with_capacity(q.len());
    for (i, &c) in lower.iter().enumerate() {
        if qi < q.len() && c == q[qi] {
            let mut s = 1;
            if is_boundary(i.checked_sub(1).map(|j| chars[j])) {
                s += 8;
            }
            match last {
                Some(l) if l + 1 == i => s += 6,
                Some(l) => s -= ((i - l - 1) as i32).min(5),
                None => s -= (i as i32).min(8),
            }
            score += s;
            last = Some(i);
            positions.push(i);
            qi += 1;
        }
    }
    (qi == q.len()).then_some(Match { score, positions })
}

/// Rank a plain label (command, skill, model): exact > prefix > word > scattered.
pub fn rank_label(query: &str, label: &str) -> Option<Match> {
    if query.is_empty() {
        return Some(Match {
            score: 0,
            positions: vec![],
        });
    }
    let (q, l) = (query.to_lowercase(), label.to_lowercase());
    let n = query.chars().count();
    if l == q {
        return Some(Match {
            score: 10_000,
            positions: (0..n).collect(),
        });
    }
    if l.starts_with(&q) {
        return Some(Match {
            score: 5_000 - label.chars().count() as i32,
            positions: (0..n).collect(),
        });
    }
    if let Some(m) = word_match(&q, label) {
        return Some(m);
    }
    subsequence(query, label)
}

/// `query` equals the start of some word inside `label` (split on non-alphanumerics).
fn word_match(q: &str, label: &str) -> Option<Match> {
    let chars: Vec<char> = label.chars().collect();
    let n = q.chars().count();
    for i in 0..chars.len() {
        if !is_boundary(i.checked_sub(1).map(|j| chars[j])) || i + n > chars.len() {
            continue;
        }
        let window: String = chars[i..i + n].iter().collect();
        if window.to_lowercase() == q {
            return Some(Match {
                score: 2_000 - i as i32,
                positions: (i..i + n).collect(),
            });
        }
    }
    None
}

/// Rank a file path. The file name counts most; a query containing `/` matches against the whole path.
pub fn rank_path(query: &str, path: &str) -> Option<Match> {
    if query.is_empty() {
        return Some(Match {
            score: 0,
            positions: vec![],
        });
    }
    let trimmed = path.trim_end_matches('/');
    let name_start_byte = trimmed.rfind('/').map_or(0, |i| i + 1);
    let name_start = trimmed[..name_start_byte].chars().count();
    let name = &trimmed[name_start_byte..];

    if query.contains('/') {
        let q = query.to_lowercase();
        let lp = path.to_lowercase();
        if lp.starts_with(&q) {
            let n = query.chars().count();
            return Some(Match {
                score: 6_000 - path.chars().count() as i32,
                positions: (0..n).collect(),
            });
        }
        return subsequence(query, path).map(|m| Match {
            score: m.score,
            positions: m.positions,
        });
    }

    if let Some(mut m) = rank_label(query, name) {
        // Shallow files beat deep ones when the name rank ties.
        m.score += 100 - (trimmed.matches('/').count() as i32).min(50) * 2;
        m.positions = m.positions.into_iter().map(|p| p + name_start).collect();
        return Some(m);
    }
    // Last resort: letters scattered across the folders and the name.
    subsequence(query, path).map(|m| Match {
        score: m.score - 200,
        positions: m.positions,
    })
}

#[cfg(test)]
mod tests;
