use super::*;
use std::path::PathBuf;

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn at_end(s: &str) -> Option<Trigger> {
    let c = chars(s);
    detect(&c, c.len())
}

#[test]
fn detects_each_trigger() {
    assert_eq!(at_end("/mo"), Some(Trigger::Slash { query: "mo".into() }));
    assert_eq!(at_end("/"), Some(Trigger::Slash { query: "".into() }));
    assert_eq!(
        at_end("fix @src/ma"),
        Some(Trigger::At {
            query: "src/ma".into(),
            start: 4
        })
    );
    assert_eq!(
        at_end("use $dep"),
        Some(Trigger::Dollar {
            query: "dep".into(),
            start: 4
        })
    );
    assert_eq!(
        at_end("/model gpt"),
        Some(Trigger::Arg {
            cmd: Cmd::Model,
            query: "gpt".into(),
            start: 7
        })
    );
    assert_eq!(
        at_end("/permissions "),
        None,
        "a bare picker command opens its picker, not a second popup"
    );
    assert_eq!(
        at_end("/skills "),
        Some(Trigger::Arg {
            cmd: Cmd::Skills,
            query: "".into(),
            start: 8
        })
    );
}

#[test]
fn ignores_non_triggers() {
    assert_eq!(at_end("mail me@example.com"), None);
    assert_eq!(at_end("costs $5 today"), None, "cursor is past the token");
    assert_eq!(
        at_end("/help me"),
        None,
        "commands without argument completion"
    );
    assert_eq!(at_end("plain text"), None);
    assert_eq!(at_end("path/with/slash"), None);
}

#[test]
fn cursor_in_the_middle_completes_the_token_before_it() {
    let c = chars("see @src/ma and more");
    assert_eq!(
        detect(&c, 11),
        Some(Trigger::At {
            query: "src/ma".into(),
            start: 4
        })
    );
    assert_eq!(detect(&c, 3), None);
}

fn sources() -> (PathBuf, Index) {
    let root = std::env::temp_dir().join(format!("rig-complete-{}", std::process::id()));
    for f in ["src/main.rs", "src/my notes.md", "docs/guide.md"] {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }
    let idx = Index::spawn(root.clone(), None);
    let t = std::time::Instant::now();
    while !idx.ready() && t.elapsed().as_secs() < 5 {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    (root, idx)
}

#[test]
fn builds_file_slash_skill_and_arg_candidates() {
    let (root, idx) = sources();
    let skills = vec![
        rig_core::skills::parse(
            "---\nname: deploy\ndescription: Ship it\n---\nb",
            "deploy",
            PathBuf::from("/s/deploy/SKILL.md"),
            "user",
        )
        .unwrap(),
        rig_core::skills::parse(
            "---\nname: review\ndescription: Review code\n---\nb",
            "review",
            PathBuf::from("/s/review/SKILL.md"),
            "user",
        )
        .unwrap(),
    ];
    let models = vec![
        ModelInfo {
            id: "gpt-6-sol".into(),
            display_name: "GPT-6 Sol".into(),
            ..Default::default()
        },
        ModelInfo {
            id: "gpt-6-luna".into(),
            display_name: "GPT-6 Luna".into(),
            hidden: true,
            ..Default::default()
        },
    ];
    let src = Sources {
        index: &idx,
        skills: &skills,
        models: &models,
        efforts: &[Effort::Low, Effort::High],
        current_model: "gpt-6-sol",
    };

    let files = build(
        Trigger::At {
            query: "my".into(),
            start: 0,
        },
        3,
        &src,
    );
    assert_eq!(files.items[0].label, "src/my notes.md");
    assert_eq!(
        files.items[0].insert, "@\"src/my notes.md\"",
        "paths with spaces are quoted"
    );
    let dirs = build(
        Trigger::At {
            query: "docs".into(),
            start: 0,
        },
        5,
        &src,
    );
    assert_eq!(
        (dirs.items[0].kind, dirs.items[0].insert.as_str()),
        (Kind::Dir, "@docs/")
    );

    let slash = build(
        Trigger::Slash {
            query: "mod".into(),
        },
        4,
        &src,
    );
    assert_eq!(
        (
            slash.items[0].label.as_str(),
            slash.items[0].insert.as_str()
        ),
        ("/model", "/model ")
    );
    assert!(
        slash.items[0].complete_command,
        "Enter on /model runs it (opens the picker)"
    );
    assert!(
        build(
            Trigger::Slash {
                query: "hel".into()
            },
            4,
            &src
        )
        .items[0]
            .complete_command
    );

    let sk = build(
        Trigger::Dollar {
            query: "rev".into(),
            start: 0,
        },
        4,
        &src,
    );
    assert_eq!(
        (sk.items.len(), sk.items[0].insert.as_str()),
        (1, "$review")
    );

    let m = build(
        Trigger::Arg {
            cmd: Cmd::Model,
            query: "".into(),
            start: 7,
        },
        7,
        &src,
    );
    assert_eq!(m.items.len(), 1, "hidden models are not suggested");
    assert!(m.items[0].detail.contains("current"));
    let e = build(
        Trigger::Arg {
            cmd: Cmd::Effort,
            query: "h".into(),
            start: 8,
        },
        9,
        &src,
    );
    assert_eq!(e.items[0].label, "high");
    let p = build(
        Trigger::Arg {
            cmd: Cmd::Permissions,
            query: "full".into(),
            start: 13,
        },
        17,
        &src,
    );
    assert_eq!(p.items[0].label, "full-access");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn selection_wraps() {
    let mut c = Completion {
        trigger: Trigger::Slash { query: "".into() },
        items: vec![],
        selected: 0,
        range: (0, 0),
        loading: false,
    };
    c.nav(1);
    assert_eq!(c.selected, 0);
    let item = Item {
        label: "a".into(),
        detail: String::new(),
        insert: "a".into(),
        positions: vec![],
        kind: Kind::Value,
        complete_command: false,
    };
    c.items = vec![item.clone(), item.clone(), item];
    c.nav(-1);
    assert_eq!(c.selected, 2);
    c.nav(1);
    assert_eq!(c.selected, 0);
}
