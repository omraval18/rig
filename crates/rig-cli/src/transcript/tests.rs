use super::*;

fn visible(s: &str) -> String {
    let mut out = String::new();
    let mut esc = false;
    for c in s.chars() {
        match (esc, c) {
            (_, '\x1b') => esc = true,
            (true, 'm') => esc = false,
            (false, c) => out.push(c),
            _ => {}
        }
    }
    out
}

#[test]
fn user_card_rows_are_full_width_and_wrap_long_and_wide_text() {
    let card = user_card(
        "first line\n\n日本語 and a rather long second paragraph that must wrap",
        30,
        true,
    );
    let rows: Vec<&str> = card.split('\n').collect();
    assert!(
        rows.len() >= 4,
        "blank line kept and long text wrapped: {rows:?}"
    );
    for r in &rows {
        assert_eq!(
            crate::view::width_of(&visible(r)),
            30,
            "every row fills the width: {r:?}"
        );
    }
    assert!(visible(rows[0]).starts_with(" › first"));
    assert!(
        visible(rows[1]).trim().is_empty(),
        "paragraph break stays inside the card"
    );
}

#[test]
fn user_card_without_color_is_plain_text() {
    assert_eq!(user_card("hi\nthere", 40, false), "› hi\n  there");
}

#[test]
fn newest_output_is_followed_and_old_output_stays_put_while_you_read_it() {
    let mut t = Transcript::new(40, false);
    for i in 0..30 {
        t.push_text(&format!("line {i}"));
    }
    assert_eq!(t.view(10).last().unwrap(), "line 29");
    t.scroll_by(5, 10);
    assert_eq!(t.view(10).last().unwrap(), "line 24");
    t.push_text("new");
    assert_eq!(
        t.view(10).last().unwrap(),
        "line 24",
        "scrolled back: the view does not jump"
    );
    t.scroll_by(-100, 10);
    assert_eq!(t.view(10).last().unwrap(), "new");
}

#[test]
fn a_short_transcript_is_top_aligned_and_resizing_rewraps_it() {
    let mut t = Transcript::new(40, false);
    t.push_text("hello");
    let v = t.view(5);
    assert_eq!((v[0].as_str(), v[4].as_str()), ("hello", ""));
    t.push_text("a long line that has to wrap when the window gets this narrow, surely");
    let before = t.row_count();
    t.set_width(20);
    assert!(t.row_count() > before);
    t.set_width(80);
    assert_eq!(t.row_count(), 2);
    t.clear();
    assert!(t.is_empty() && t.view(3).iter().all(String::is_empty));
}
