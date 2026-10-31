use super::*;

fn items() -> Vec<PickItem> {
    ["gpt-6-sol", "gpt-6-luna", "claude-sonnet"]
        .iter()
        .enumerate()
        .map(|(i, n)| {
            PickItem::labeled(*n)
                .detail(format!("model {i}"))
                .current(*n == "gpt-6-luna")
        })
        .collect()
}

#[test]
fn opens_on_the_current_item_and_filters_as_you_type() {
    let mut p = Picker::new(PickKind::Model, "Model", items());
    assert_eq!(p.selected_item().unwrap().value, "gpt-6-luna");
    for c in "sonn".chars() {
        p.type_char(c);
    }
    assert_eq!(p.len(), 1);
    assert_eq!(p.selected_item().unwrap().value, "claude-sonnet");
    for _ in 0..4 {
        p.backspace();
    }
    assert_eq!(p.len(), 3);
}

#[test]
fn navigation_wraps_and_empty_filter_result_is_safe() {
    let mut p = Picker::new(PickKind::Model, "Model", items());
    p.nav(-1);
    p.nav(-1);
    p.nav(-1);
    assert!(p.selected_item().is_some());
    for c in "zzzz".chars() {
        p.type_char(c);
    }
    assert!(p.is_empty() && p.selected_item().is_none());
    p.nav(1);
}

#[test]
fn rows_window_follows_the_selection() {
    let many: Vec<PickItem> = (0..30)
        .map(|i| PickItem::labeled(format!("m{i}")))
        .collect();
    let mut p = Picker::new(PickKind::Resume, "x", many);
    for _ in 0..15 {
        p.nav(1);
    }
    let rows = p.rows(8);
    assert_eq!(rows.len(), 8);
    assert!(rows.iter().any(|r| r.2) && rows.last().unwrap().0.value == "m15");
}
