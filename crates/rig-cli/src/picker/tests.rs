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
    assert!(matches!(
        rows.last(),
        Some(Row::Item { item, selected: true, .. }) if item.value == "m15"
    ));
}

fn grouped() -> Picker {
    let row = |label: &str, group: &str| PickItem::labeled(label).group(group);
    Picker::new(
        PickKind::Model,
        "x",
        vec![
            row("chatgpt/gpt-6-sol", "chatgpt"),
            row("chatgpt/gpt-6-luna", "chatgpt"),
            row("openrouter/openai/gpt-6-sol", "openrouter"),
        ],
    )
}

#[test]
fn headings_show_until_you_type_and_arrows_jump_between_groups() {
    let mut p = grouped();
    let headings = |p: &Picker| {
        p.rows(20)
            .iter()
            .filter(|r| matches!(r, Row::Heading(_)))
            .count()
    };
    assert_eq!(headings(&p), 2);
    p.nav(1);
    p.nav_group(true);
    assert_eq!(p.position(), "3/3");
    p.nav_group(false);
    assert_eq!(p.position(), "1/3");
    p.set_filter("openrouter sol");
    assert_eq!((headings(&p), p.len()), (0, 1), "every word must match");
}

#[test]
fn a_search_for_an_unlisted_provider_is_not_a_dead_end() {
    let mut p = Picker::new(PickKind::Provider, "x", vec![PickItem::labeled("openai")]);
    p.set_create(Create::Endpoint);
    p.set_filter("openai");
    assert_eq!(p.len(), 1);
    p.set_filter("zzz");
    assert_eq!(p.selected_item().unwrap().value, "\t");
    p.set_filter("https://api.example.com/v1");
    assert_eq!(
        p.selected_item().unwrap().value,
        "\thttps://api.example.com/v1"
    );
}

#[test]
fn a_refreshed_list_keeps_the_highlighted_item() {
    let mut p = grouped();
    p.nav(1);
    p.replace_items(vec![
        PickItem::labeled("chatgpt/gpt-7"),
        PickItem::labeled("chatgpt/gpt-6-luna"),
    ]);
    assert_eq!(p.selected_item().unwrap().value, "chatgpt/gpt-6-luna");
}

#[test]
fn typing_an_unknown_profile_offers_to_create_it() {
    let mut p = Picker::new(PickKind::Profile, "x", vec![PickItem::labeled("default")]);
    p.set_create(Create::Profile {
        copy_from: "default".into(),
    });
    assert_eq!(p.len(), 1);
    p.set_filter("work");
    assert_eq!(p.len(), 2);
    assert_eq!(p.selected_item().unwrap().value, "\twork");
    p.set_filter("default");
    assert_eq!(p.len(), 1, "an existing name is switched to, not created");
    p.set_filter("not valid!");
    assert!(p.is_empty());
}
