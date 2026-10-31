use super::*;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "rig-sess-{tag}-{}-{}",
        std::process::id(),
        now_nanos()
    ));
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn roundtrip_title_and_listing() {
    let root = tmp("a");
    let cwd = Path::new("/work/proj");
    let mut s = Session::create(&root, cwd, "m").unwrap();
    s.append(&Message::user("<rig-context>env</rig-context>"))
        .unwrap();
    s.append(&Message::user("fix the parser\nplease")).unwrap();
    let msgs = Session::load(&s.path).unwrap();
    assert_eq!(msgs.len(), 2);
    let listed = Session::list(&root, cwd);
    assert_eq!(
        (listed.len(), listed[0].title.as_str(), listed[0].messages),
        (1, "fix the parser", 2)
    );
    s.set_title("Parser work").unwrap();
    assert_eq!(Session::list(&root, cwd)[0].title, "Parser work");
    assert_eq!(Session::latest(&root, cwd), Some(s.path.clone()));
    assert_eq!(Session::find(&root, cwd, &s.id[..4]), Some(s.path.clone()));
    fs::remove_dir_all(root).ok();
}

#[test]
fn checkpoints_replace_a_range_on_load() {
    let root = tmp("b");
    let mut s = Session::create(&root, Path::new("/p"), "m").unwrap();
    for t in ["env", "a", "b", "c", "d"] {
        s.append(&Message::user(t)).unwrap();
    }
    s.append_checkpoint(4, &Message::user("SUMMARY")).unwrap();
    s.append(&Message::user("e")).unwrap();
    let texts: Vec<_> = Session::load(&s.path)
        .unwrap()
        .iter()
        .map(Message::text)
        .collect();
    assert_eq!(texts, ["env", "SUMMARY", "d", "e"]);
    fs::remove_dir_all(root).ok();
}

#[test]
fn fork_copies_messages_and_archive() {
    let root = tmp("c");
    let cwd = Path::new("/p");
    let mut s = Session::create(&root, cwd, "m").unwrap();
    s.persist().unwrap();
    fs::write(s.archive(), "{\"id\":\"M1\"}\n").unwrap();
    let f = Session::fork(
        &root,
        cwd,
        "m",
        &s.path,
        &[Message::user("env"), Message::user("one")],
    )
    .unwrap();
    assert_eq!(Session::load(&f.path).unwrap().len(), 2);
    assert!(f.archive().is_file());
    assert_ne!(f.id, s.id);
    fs::remove_dir_all(root).ok();
}

#[test]
fn a_session_remembers_its_own_settings_and_old_ones_fall_back_to_their_model() {
    let root = std::env::temp_dir().join(format!("rig-sess-settings-{}", std::process::id()));
    let cwd = root.join("proj");
    std::fs::create_dir_all(&cwd).unwrap();
    let mut a = Session::create(&root, &cwd, "chatgpt/gpt-6-sol").unwrap();
    a.persist().unwrap();
    assert_eq!(
        Session::settings(&a.path),
        SessionSettings {
            model: Some("chatgpt/gpt-6-sol".into()),
            effort: None,
            autonomy: None
        }
    );
    a.append_settings("anthropic/claude-sonnet-5-5", Some("high"), "medium")
        .unwrap();
    let mut b = Session::create(&root, &cwd, "chatgpt/gpt-6.1-terra").unwrap();
    b.persist().unwrap();
    b.append_settings("chatgpt/gpt-6.1-terra", None, "ask")
        .unwrap();
    a.append_settings("anthropic/claude-sonnet-5-5", Some("low"), "high")
        .unwrap();
    assert_eq!(
        Session::settings(&a.path),
        SessionSettings {
            model: Some("anthropic/claude-sonnet-5-5".into()),
            effort: Some("low".into()),
            autonomy: Some("high".into())
        }
    );
    assert_eq!(Session::settings(&b.path).autonomy.as_deref(), Some("ask"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn sessions_saved_with_the_old_context_tag_still_list_by_what_the_user_typed() {
    let root = tmp("legacy");
    let cwd = Path::new("/work/proj");
    let mut s = Session::create(&root, cwd, "m").unwrap();
    s.append(&Message::user("<system-reminder>\nenv\n</system-reminder>"))
        .unwrap();
    s.append(&Message::user("fix the parser")).unwrap();
    assert_eq!(Session::list(&root, cwd)[0].title, "fix the parser");
}

#[test]
fn a_session_reaches_the_disk_only_once_someone_says_something() {
    let root = tmp("lazy");
    let cwd = Path::new("/p");
    let mut s = Session::create(&root, cwd, "m").unwrap();
    s.append(&Message::user("<rig-context>env</rig-context>"))
        .unwrap();
    s.append_settings("m", None, "low").unwrap();
    assert!(
        !s.path.exists() && !s.is_saved(),
        "context alone is not saved"
    );
    s.append(&Message::user("hello")).unwrap();
    assert!(s.is_saved());
    let texts: Vec<_> = Session::load(&s.path)
        .unwrap()
        .iter()
        .map(Message::text)
        .collect();
    assert_eq!(texts, ["<rig-context>env</rig-context>", "hello"]);
    assert_eq!(Session::settings(&s.path).autonomy.as_deref(), Some("low"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn recording_the_same_settings_twice_writes_once() {
    let root = tmp("dedupe");
    let mut s = Session::create(&root, Path::new("/p"), "m").unwrap();
    s.append(&Message::user("hi")).unwrap();
    for _ in 0..3 {
        s.append_settings("m", Some("high"), "low").unwrap();
    }
    let lines = |p: &Path| fs::read_to_string(p).unwrap().lines().count();
    assert_eq!(lines(&s.path), 3, "meta, message, one settings record");
    let mut again = Session::reopen(&s.path).unwrap();
    again.append_settings("m", Some("high"), "low").unwrap();
    assert_eq!(
        lines(&s.path),
        3,
        "a reopened session knows what it recorded"
    );
    again.append_settings("m", Some("low"), "low").unwrap();
    assert_eq!(lines(&s.path), 4);
    fs::remove_dir_all(root).ok();
}
