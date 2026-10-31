use super::*;

fn plain(s: &str) -> String {
    let mut out = String::new();
    for p in pieces(s) {
        if let Piece::Ch(c, _) = p {
            out.push(c);
        }
    }
    out
}

#[test]
fn short_lines_are_untouched() {
    assert_eq!(wrap("hello", 20), ["hello"]);
    assert_eq!(visible_width("\x1b[1mhi\x1b[0m"), 2);
}

#[test]
fn breaks_at_spaces_not_inside_words() {
    let rows = wrap("the quick brown fox jumps over the lazy dog", 16);
    assert_eq!(rows, ["the quick brown", "fox jumps over", "the lazy dog"]);
    assert!(rows.iter().all(|r| visible_width(r) <= 16));
}

#[test]
fn a_word_longer_than_the_row_is_split_hard() {
    let rows = wrap("aaaaaaaaaaaaaaaaaaaaaaaa", 10);
    assert!(
        rows.len() == 3 && rows.iter().all(|r| visible_width(r) <= 10),
        "{rows:?}"
    );
    assert_eq!(rows.concat(), "aaaaaaaaaaaaaaaaaaaaaaaa");
}

#[test]
fn styles_continue_onto_the_next_row_and_are_closed_on_every_row() {
    let rows = wrap(
        "\x1b[1mbold text that is long enough to wrap\x1b[0m done",
        14,
    );
    assert!(rows.len() >= 3);
    for r in &rows[..rows.len() - 1] {
        assert!(r.ends_with(RESET), "{r:?}");
    }
    assert!(
        rows[1].starts_with("\x1b[1m"),
        "the second row is still bold: {:?}",
        rows[1]
    );
    assert_eq!(
        plain(&rows.join(" "))
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        "bold text that is long enough to wrap done"
    );
}

#[test]
fn list_items_wrap_with_a_hanging_indent() {
    let rows = wrap("- first item that goes on and on for a while", 16);
    assert!(rows[0].starts_with("- first"));
    assert!(
        rows[1].starts_with("  ") && !rows[1].starts_with("   "),
        "{rows:?}"
    );
}

#[test]
fn wide_characters_count_two_columns() {
    let rows = wrap("日本語日本語日本語", 8);
    assert!(rows.iter().all(|r| visible_width(r) <= 8), "{rows:?}");
}

#[test]
fn clipping_closes_styles() {
    let c = clip("\x1b[31mabcdefghij\x1b[0m", 4);
    assert_eq!(plain(&c), "abcd");
    assert!(c.ends_with(RESET));
    assert_eq!(clip("short", 10), "short");
}
