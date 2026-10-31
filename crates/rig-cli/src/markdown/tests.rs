use super::*;

fn strip(s: &str) -> String {
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

fn shown(input: &str) -> String {
    strip(&Styler::new(true).line(input))
}

#[test]
fn markers_disappear_and_text_stays() {
    for (input, visible) in [
        ("# Title", "Title"),
        ("### Sub ###", "Sub"),
        ("plain text", "plain text"),
        ("use `cargo test` now", "use cargo test now"),
        (
            "a **bold** and *it* and _it_ and ~~gone~~",
            "a bold and it and it and gone",
        ),
        ("``a ` b``", "a ` b"),
        ("**bold `code`**", "bold code"),
        ("***both***", "both"),
        ("> quoted **b**", "│ quoted b"),
        ("- item", "• item"),
        ("  - nested", "  ◦ nested"),
        ("+ plus", "• plus"),
        ("- [x] done", "• ☑ done"),
        ("- [ ] todo", "• ☐ todo"),
        ("1. one", "1. one"),
        ("[docs](https://x.y/z)", "docs (https://x.y/z)"),
        ("[https://a.b](https://a.b)", "https://a.b"),
        (r"\*lit\* and \`x\`", "*lit* and `x`"),
        ("| a | `b` |", "│ a │ b │"),
        ("|---|:-:|", "│───│───│"),
        ("日本 **語** é**ü**ñ", "日本 語 éüñ"),
    ] {
        assert_eq!(shown(input), visible, "{input}");
    }
}

#[test]
fn text_that_only_looks_like_markup_is_left_alone() {
    for input in [
        "#hashtag",
        "####### seven",
        "unclosed `tick",
        "2 ** 3",
        "2 * 3 * 4",
        "snake_case_name and __dunder",
        "**unclosed bold",
        "a ~ b ~~ c",
        "[not a link] (really)",
        "[](x)",
        "a trailing backslash\\",
        "price: $5 * 2 = 10 * 3",
    ] {
        assert_eq!(shown(input), input, "{input}");
    }
}

#[test]
fn spans_close_and_restore_the_surrounding_style() {
    let mut s = Styler::new(true);
    assert_eq!(s.line("**a** b"), format!("{BOLD}a{RESET} b"));
    assert_eq!(s.line("x `c` y"), format!("x {CYAN}c{RESET} y"));
    // Inside a quote the dim comes back after the code span.
    let q = s.line("> a `c` b");
    assert!(q.contains(&format!("{CYAN}c{RESET}{DIM}")), "{q:?}");
    // A heading stays bold around bold text.
    let h = s.line("## a **b** c");
    assert!(
        h.starts_with(BOLD) && h.contains(&format!("{RESET}{BOLD} c")),
        "{h:?}"
    );
    assert!(s.line("# big").starts_with(H1));
}

#[test]
fn code_fences_style_everything_inside() {
    for fence in ["```", "~~~"] {
        let mut s = Styler::new(true);
        assert!(s.line(&format!("{fence}rust")).contains(DIM));
        let inside = s.line("# let x = `a` * **b**;");
        assert_eq!(strip(&inside), "# let x = `a` * **b**;");
        assert!(inside.starts_with(CYAN) && !inside.contains(BOLD));
        s.line(fence);
        assert!(!s.line("after fence").contains(CYAN));
    }
}

#[test]
fn rules_are_drawn() {
    for r in ["---", "***", "___"] {
        assert_eq!(shown(r), "─".repeat(RULE), "{r}");
    }
    assert_eq!(shown("- - x"), "• - x");
}

#[test]
fn no_color_is_passthrough() {
    let mut s = Styler::new(false);
    assert_eq!(s.line("# **hi** `x`"), "# **hi** `x`");
}

#[test]
fn deeply_nested_markers_do_not_recurse_forever() {
    let nasty = "*a **b *c **d *e *f* e* d** c* b** a*".repeat(50);
    assert!(!shown(&nasty).is_empty());
}
