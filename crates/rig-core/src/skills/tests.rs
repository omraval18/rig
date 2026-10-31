use super::*;

#[test]
fn the_catalog_stays_within_budget_and_keeps_the_first_skills() {
    let many: Vec<Skill> = (0..150)
        .map(|i| Skill {
            name: format!("skill-{i}"),
            description: "does a very long list of useful things ".repeat(8),
            path: PathBuf::from("/x"),
            scope: "user",
        })
        .collect();
    let text = catalog_text(&many);
    assert!(text.len() <= CATALOG_BUDGET + 300, "{}", text.len());
    assert!(text.contains("skill-0:") && text.contains("more skills not listed"));
    let few = catalog_text(&many[..3]);
    assert!(!few.contains("not listed") && few.contains("skill-2"));
}

fn mk(text: &str, fallback: &str) -> Option<Skill> {
    parse(text, fallback, PathBuf::from("/x/SKILL.md"), "project")
}

#[test]
fn parses_frontmatter_and_quotes() {
    let s = mk(
        "---\nname: deploy\ndescription: \"Ship it safely\"\n---\n# Deploy\nsteps",
        "dir",
    )
    .unwrap();
    assert_eq!(
        (s.name.as_str(), s.description.as_str()),
        ("deploy", "Ship it safely")
    );
}

#[test]
fn folded_description_and_name_fallback() {
    let s = mk(
        "---\ndescription: >\n  Use when the user asks\n  to review code.\n---\nbody",
        "reviewer",
    )
    .unwrap();
    assert_eq!(s.name, "reviewer");
    assert_eq!(s.description, "Use when the user asks to review code.");
}

#[test]
fn no_frontmatter_uses_first_line() {
    let s = mk("# Title\n\nDoes the thing well.\n", "thing").unwrap();
    assert_eq!(s.description, "Does the thing well.");
}

#[test]
fn rejects_bad_names() {
    assert!(mk("---\nname: has space\n---\nx", "d").is_none());
}

#[test]
fn body_strips_frontmatter_and_project_beats_user() {
    let base = std::env::temp_dir().join(format!("rig-skills-{}", std::process::id()));
    let proj = base.join("proj");
    let home = base.join("home");
    for (dir, text) in [
        (
            proj.join(".claude/skills/a"),
            "---\nname: a\ndescription: project a\n---\nPROJECT",
        ),
        (
            home.join(".rig/skills/a"),
            "---\nname: a\ndescription: user a\n---\nUSER",
        ),
        (
            home.join(".rig/skills/b"),
            "---\nname: b\ndescription: only user\n---\nB",
        ),
    ] {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), text).unwrap();
    }
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    let found = discover(&proj, Some(&home));
    assert_eq!(
        found
            .iter()
            .map(|s| (s.name.as_str(), s.description.as_str()))
            .collect::<Vec<_>>(),
        [("a", "project a"), ("b", "only user")]
    );
    assert_eq!(body(&found[0]).unwrap().trim(), "PROJECT");
    assert!(
        activation_text(&found[0])
            .unwrap()
            .contains("Skill \"a\" is active")
    );
    std::fs::remove_dir_all(base).ok();
}
