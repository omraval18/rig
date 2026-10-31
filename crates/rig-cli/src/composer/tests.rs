use super::*;

fn c(s: &str) -> Composer {
    let mut c = Composer::default();
    c.insert_str(s);
    c
}

#[test]
fn insert_delete_and_unicode() {
    let mut c = c("héllo");
    c.left();
    c.backspace();
    assert_eq!(c.text(), "hélo");
    c.home();
    c.delete();
    assert_eq!(c.text(), "élo");
    assert_eq!(c.cursor, 0);
}

#[test]
fn word_motion_and_kill_ring() {
    let mut c = c("foo bar_baz  qux");
    c.word_left();
    assert_eq!(c.cursor, 13);
    c.word_left();
    assert_eq!(c.cursor, 4);
    c.kill_to_end();
    assert_eq!(c.text(), "foo ");
    c.home();
    c.yank();
    assert_eq!(c.text(), "bar_baz  quxfoo ");
    let mut d = Composer::default();
    d.insert_str("one two");
    d.kill_word_back();
    assert_eq!(d.text(), "one ");
    d.yank();
    assert_eq!(d.text(), "one two");
}

#[test]
fn multiline_cursor_keeps_column() {
    let mut c = c("abcdef\nxy\nlonger line");
    assert!(c.up());
    assert_eq!(c.cursor, 7 + 2, "clamped to the short line's end");
    assert!(c.up());
    assert!(!c.up());
    assert!(c.down() && c.down());
    assert!(!c.down());
    c.home();
    assert_eq!(c.chars()[c.cursor], 'l');
}

#[test]
fn history_navigation_preserves_the_draft() {
    let mut c = Composer::new(vec!["first".into(), "second".into()]);
    c.insert_str("draft");
    assert!(c.history_prev());
    assert_eq!(c.text(), "second");
    assert!(c.history_prev());
    assert_eq!(c.text(), "first");
    assert!(!c.history_prev());
    assert!(c.history_next() && c.history_next());
    assert_eq!(c.text(), "draft");
    assert!(!c.history_next());
}

#[test]
fn submit_records_history_once_and_clears() {
    let mut c = c("do it  ");
    assert_eq!(c.submit(), "do it");
    assert!(c.is_empty());
    c.insert_str("do it");
    c.submit();
    assert_eq!(c.history, ["do it"]);
}

#[test]
fn paste_normalizes_line_endings_and_strips_control_codes() {
    let c = c("a\r\nb\u{1b}[31m\rc");
    assert_eq!(c.text(), "a\nb[31m\nc");
}

#[test]
fn backslash_continuation_and_replace_range() {
    let mut c = c("first \\");
    assert!(c.continues_line());
    let mut d = c.text().chars().collect::<Vec<_>>();
    d.pop();
    c.replace_range(0, 7, "x ");
    assert_eq!(c.text(), "x ");
    assert_eq!(c.cursor, 2);
}
