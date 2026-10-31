//! Small text helpers shared by tools and the agent loop.

/// `1 file`, `3 files`.
pub fn count(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// Whether `bytes` look like a binary file: a NUL in the first 8 KiB.
pub fn looks_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8192)].contains(&0)
}

/// Keep the head and tail of `s` within `max` bytes, naming how much was cut.
pub fn head_tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let half = max / 2;
    let mut head = half;
    while !s.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = s.len() - half;
    while !s.is_char_boundary(tail) {
        tail += 1;
    }
    format!(
        "{}\n[... {} bytes cut ...]\n{}",
        &s[..head],
        tail - head,
        &s[tail..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_tail_cuts_the_middle_on_char_boundaries() {
        assert_eq!(head_tail("short", 100), "short");
        let cut = head_tail(&"é".repeat(100), 21);
        assert!(cut.starts_with('é') && cut.ends_with('é') && cut.contains("bytes cut"));
    }

    #[test]
    fn count_pluralizes() {
        assert_eq!(count(1, "file"), "1 file");
        assert_eq!(count(0, "file"), "0 files");
    }

    #[test]
    fn binary_means_a_nul_early_on() {
        assert!(looks_binary(&[1, 0, 2]));
        assert!(!looks_binary(b"plain text"));
    }
}
