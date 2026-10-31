use super::*;

fn toks(s: &str) -> Vec<(Kind, String)> {
    find_tokens(s)
        .into_iter()
        .map(|t| (t.kind, t.value))
        .collect()
}

#[test]
fn finds_mentions_with_boundaries_and_punctuation() {
    assert_eq!(
        toks("look at @src/main.rs, then $deploy."),
        [
            (Kind::At, "src/main.rs".into()),
            (Kind::Dollar, "deploy".into())
        ]
    );
    assert_eq!(toks("mail me@example.com costs$5"), []);
    assert_eq!(
        toks("see (@a/b.txt) and @\"my file.md\""),
        [
            (Kind::At, "a/b.txt".into()),
            (Kind::At, "my file.md".into())
        ]
    );
    assert_eq!(
        toks("@.. and @~"),
        [(Kind::At, "..".into()), (Kind::At, "~".into())]
    );
}

#[test]
fn token_spans_are_char_indexes() {
    let t = &find_tokens("é @x y")[0];
    assert_eq!((t.start, t.end), (2, 4));
}

#[test]
fn expands_files_dirs_skills_and_reports_missing() {
    let base = std::env::temp_dir().join(format!("rig-expand-{}", std::process::id()));
    std::fs::create_dir_all(base.join("sub")).unwrap();
    std::fs::write(base.join("a.txt"), "hello\nworld\n").unwrap();
    std::fs::write(base.join("big.txt"), "x".repeat(MAX_INLINE_FILE + 1)).unwrap();
    std::fs::write(base.join("bin.dat"), [0u8, 1, 2]).unwrap();
    let skill_dir = base.join("sk");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: go\ndescription: d\n---\nDo the go thing",
    )
    .unwrap();
    let sk = skills::parse(
        "---\nname: go\ndescription: d\n---\nDo the go thing",
        "go",
        skill_dir.join("SKILL.md"),
        "project",
    )
    .unwrap();

    let ex = expand(
        "review @a.txt @sub @big.txt @bin.dat @nope.rs and $go but not $HOME",
        &base,
        None,
        &[sk],
    );
    assert_eq!((ex.files.len(), ex.dirs.len(), ex.skills.len()), (3, 1, 1));
    assert_eq!(ex.missing, ["@nope.rs"]);
    let all = ex.reminders.join("\n");
    assert!(
        all.contains("hello\nworld")
            && all.contains("not inlined")
            && all.contains("binary file")
            && all.contains("Do the go thing")
    );
    assert!(ex.summary().contains("3 files") && ex.summary().contains("skill go"));
    std::fs::remove_dir_all(base).ok();
}

#[test]
fn an_image_mention_attaches_pixels_instead_of_inlining_bytes() {
    let base = std::env::temp_dir().join(format!("rig-expand-img-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("shot.png"), [0x89, b'P', b'N', b'G', 1, 2, 3]).unwrap();
    let ex = expand("what is in @shot.png", &base, None, &[]);
    assert_eq!(ex.images.len(), 1);
    assert!(
        matches!(&ex.images[0], crate::message::Block::Image { mime, .. } if mime == "image/png")
    );
    assert!(ex.reminders.is_empty(), "no file body is inlined");
    assert_eq!(ex.summary(), "1 image");
    std::fs::remove_dir_all(base).ok();
}
