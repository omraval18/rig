use super::prompt::MAX_COMPOSER_ROWS;
use super::*;
use crate::complete::{Completion, Item, Trigger};
use crate::complete::{Kind, MAX_ROWS};
use crate::style::{CYAN, GREEN, YELLOW};

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            for n in it.by_ref() {
                if n == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn view<'a>(c: &'a Composer, comp: Option<&'a Completion>, w: usize) -> View<'a> {
    View {
        width: w,
        color: true,
        composer: c,
        completion: comp,
        picker: None,
        entry: None,
        approval: None,
        partial: "",
        activity: None,
        notice: None,
        tool_rows: vec![],
        footer_left: vec![("gpt-6-sol · medium · low".into(), "")],
        footer_right: vec![("12% ctx".into(), "")],
        is_skill: &|_| false,
    }
}

#[test]
fn wrap_breaks_on_width_and_newlines() {
    let t: Vec<char> = "abcdefghij".chars().collect();
    assert_eq!(wrap(&t, 4), [(0, 4), (4, 8), (8, 10)]);
    let t: Vec<char> = "ab\ncd".chars().collect();
    assert_eq!(wrap(&t, 10), [(0, 2), (3, 5)]);
    let t: Vec<char> = "abcd".chars().collect();
    assert_eq!(
        wrap(&t, 4),
        [(0, 4), (4, 4)],
        "exactly full row gets a cursor row"
    );
    let t: Vec<char> = "ab\n".chars().collect();
    assert_eq!(wrap(&t, 10), [(0, 2), (3, 3)]);
    let wide: Vec<char> = "日本語".chars().collect();
    assert_eq!(
        wrap(&wide, 4),
        [(0, 2), (2, 3)],
        "two double-width chars fill a 4-column row"
    );
}

#[test]
fn cursor_position_follows_wrapping_and_wide_chars() {
    let mut c = Composer::default();
    c.insert_str("hello world, this is long");
    let f = render(&view(&c, None, 20));
    assert!(
        f.lines.len() >= 6,
        "gap + rule + 2 prompt rows + rule + footer"
    );
    assert_eq!(f.cursor.0, 3, "cursor on the second prompt row");
    let mut c2 = Composer::default();
    c2.insert_str("日本");
    assert_eq!(
        render(&view(&c2, None, 40)).cursor.1,
        2 + 4,
        "wide chars take two columns"
    );
}

#[test]
fn no_row_ever_exceeds_the_terminal_width() {
    let mut c = Composer::default();
    c.insert_str(
        "fix @src/very/long/path/that/goes/on/and/on/forever.rs and $skill 日本語のテキスト /x",
    );
    let items: Vec<Item> = (0..14)
        .map(|i| Item {
            label: format!("src/some/deeply/nested/directory/file_number_{i}.rs"),
            detail: "a very long description that must be cut to fit".into(),
            insert: String::new(),
            positions: vec![0, 4, 9],
            kind: Kind::File,
            complete_command: false,
        })
        .collect();
    let comp = Completion {
        trigger: Trigger::At {
            query: "f".into(),
            start: 4,
        },
        items,
        selected: 11,
        range: (0, 0),
        loading: false,
    };
    for w in [20, 33, 80, 140] {
        let f = render(&view(&c, Some(&comp), w));
        for l in &f.lines {
            assert!(
                width_of(&strip_ansi(l)) <= w,
                "width {w}: {:?} is {} cols",
                strip_ansi(l),
                width_of(&strip_ansi(l))
            );
        }
        assert!(f.cursor.0 < f.lines.len());
    }
}

#[test]
fn popup_shows_at_most_ten_rows_and_scrolls_to_the_selection() {
    let c = Composer::default();
    let items: Vec<Item> = (0..25)
        .map(|i| Item {
            label: format!("item{i}"),
            detail: String::new(),
            insert: String::new(),
            positions: vec![],
            kind: Kind::Value,
            complete_command: false,
        })
        .collect();
    let comp = Completion {
        trigger: Trigger::Slash {
            query: String::new(),
        },
        items,
        selected: 18,
        range: (0, 0),
        loading: false,
    };
    let f = render(&view(&c, Some(&comp), 60));
    let shown: Vec<String> = f
        .lines
        .iter()
        .map(|l| strip_ansi(l))
        .filter(|l| l.contains("item"))
        .collect();
    assert_eq!(shown.len(), MAX_ROWS);
    assert!(shown.last().unwrap().contains("item18") && shown.last().unwrap().starts_with('›'));
}

#[test]
fn tall_prompts_are_windowed_around_the_cursor() {
    let mut c = Composer::default();
    c.insert_str(
        &(0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let f = render(&view(&c, None, 60));
    assert!(
        f.lines.len() <= MAX_COMPOSER_ROWS + 5,
        "gap + rule + overflow note + rows + rule + footer"
    );
    assert!(strip_ansi(&f.lines[f.cursor.0]).contains("line 39"));
}

#[test]
fn mentions_and_commands_are_colored_but_text_is_unchanged() {
    let mut c = Composer::default();
    c.insert_str("/model x @a.rs $go");
    let known = |n: &str| n == "go";
    let mut v = view(&c, None, 80);
    v.is_skill = &known;
    let f = render(&v);
    let row = &f.lines[2];
    assert!(row.contains(GREEN) && row.contains(CYAN) && row.contains(YELLOW));
    assert_eq!(strip_ansi(row), "› /model x @a.rs $go");
}

#[test]
fn truncate_adds_an_ellipsis_within_budget() {
    assert_eq!(truncate("abcdef", 4), "abc…");
    assert_eq!(truncate("abc", 4), "abc");
    assert!(width_of(&truncate("日本語日本語", 5)) <= 5);
}

#[test]
fn a_typed_key_is_drawn_as_bullets_and_the_cursor_follows_it() {
    let e = crate::entry::Entry {
        purpose: crate::entry::Purpose::NewProfile,
        title: "API key for anthropic".into(),
        hint: "saved in profile 'work'".into(),
        value: "sk-ant-secret-1234".into(),
        secret: true,
    };
    let (rows, cursor) = super::entry::rows(&e, 60, false);
    assert!(
        !rows.join("\n").contains("sk-ant"),
        "the key is never drawn"
    );
    assert_eq!(rows[1].matches('•').count(), 18);
    assert_eq!(cursor, (1, rows[1].chars().count()));
}
