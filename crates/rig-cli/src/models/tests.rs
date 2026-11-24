use super::*;

fn spec() -> ProviderSpec {
    rig_provider::builtin_specs()
        .into_iter()
        .find(|s| s.name == "openai")
        .unwrap()
}

fn model(id: &str) -> ModelInfo {
    ModelInfo {
        id: id.into(),
        ..ModelInfo::default()
    }
}

fn store(tag: &str) -> (PathBuf, Models) {
    let dir = std::env::temp_dir().join(format!("rig-models-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let m = Models::load(&dir.join("models.json"));
    (dir, m)
}

#[test]
fn lists_are_kept_and_new_models_are_reported() {
    let (dir, mut m) = store("keep");
    let s = spec();
    assert!(m.is_stale(&s));
    let n = m.begin(&s);
    assert!(!m.is_stale(&s), "a fetch is in flight");
    let first = m
        .finish("openai", n, Ok(vec![model("a")]))
        .unwrap()
        .unwrap();
    assert!(first.added.is_empty(), "a first list has nothing new in it");
    let n = m.begin(&s);
    let next = m.finish("openai", n, Ok(vec![model("a"), model("b")]));
    assert_eq!(next.unwrap().unwrap().added, ["b"]);
    let again = Models::load(&dir.join("models.json"));
    assert_eq!(again.of(&s).len(), 2);
    let moved = ProviderSpec {
        base_url: "https://elsewhere".into(),
        ..s
    };
    assert!(again.of(&moved).is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_late_answer_is_dropped_and_a_failure_keeps_the_list() {
    let (dir, mut m) = store("late");
    let s = spec();
    let old = m.begin(&s);
    m.forget("openai");
    let new = m.begin(&s);
    assert!(m.finish("openai", old, Ok(vec![model("old")])).is_none());
    m.finish("openai", new, Ok(vec![model("new")]));
    let n = m.begin(&s);
    assert!(
        m.finish("openai", n, Err("offline".into()))
            .unwrap()
            .is_err()
    );
    assert_eq!(m.of(&s)[0].id, "new");
    assert!(m.is_stale(&s), "a failed refresh is tried again");
    std::fs::remove_dir_all(dir).ok();
}
