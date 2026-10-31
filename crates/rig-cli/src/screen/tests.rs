use super::*;

fn rows(r: &[&str]) -> Vec<String> {
    r.iter().map(ToString::to_string).collect()
}

fn written(s: &Screen<Vec<u8>>) -> String {
    String::from_utf8_lossy(&s.out).into_owned()
}

#[test]
fn only_rows_that_changed_are_rewritten() {
    let mut s = Screen::new(Vec::new());
    s.paint(&rows(&["one", "two", "three"]), 20, Some((2, 1)))
        .unwrap();
    s.out.clear();
    s.paint(&rows(&["one", "TWO", "three"]), 20, Some((2, 1)))
        .unwrap();
    let o = written(&s);
    assert!(
        o.contains("TWO") && !o.contains("one") && !o.contains("three"),
        "{o:?}"
    );
}

#[test]
fn the_cursor_is_hidden_while_painting_and_shown_once_in_place() {
    let mut s = Screen::new(Vec::new());
    s.paint(&rows(&["a", "b"]), 10, Some((1, 3))).unwrap();
    let o = written(&s);
    let hide = o.find("\x1b[?25l").unwrap();
    let place = o.find("\x1b[2;4H").unwrap();
    let show = o.rfind("\x1b[?25h").unwrap();
    assert!(hide < place && place < show, "{o:?}");
    assert_eq!(o.matches("\x1b[?25h").count(), 1);
}

#[test]
fn long_rows_are_clipped_and_a_height_change_repaints_everything() {
    let mut s = Screen::new(Vec::new());
    s.paint(&rows(&["abcdefghijkl", "x"]), 5, None).unwrap();
    assert!(written(&s).contains("abcde") && !written(&s).contains("abcdef"));
    s.out.clear();
    s.paint(&rows(&["abcdefghijkl", "x", "y"]), 5, None)
        .unwrap();
    assert!(written(&s).contains("abcde") && written(&s).contains("\x1b[2J"));
}
