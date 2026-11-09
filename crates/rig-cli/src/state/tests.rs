use super::*;

#[test]
fn mouse_capture_is_enabled_by_default() {
    assert!(State::default().mouse_enabled());
    for json in ["{}", r#"{"mouse":null}"#, r#"{"mouse":true}"#] {
        let state: State = serde_json::from_str(json).unwrap();
        assert!(state.mouse_enabled(), "{json}");
    }
    let mut state: State = serde_json::from_str(r#"{"mouse":true}"#).unwrap();
    assert!(state.mouse_enabled());
    state.mouse = Some(!state.mouse_enabled());
    assert!(!state.mouse_enabled());
    let saved = serde_json::to_string(&state).unwrap();
    assert!(
        !serde_json::from_str::<State>(&saved)
            .unwrap()
            .mouse_enabled()
    );
}

#[test]
fn round_trips_and_tolerates_junk() {
    let dir = std::env::temp_dir().join(format!("rig-state-{}", std::process::id()));
    let path = dir.join("state.json");
    let s = State {
        model: Some("chatgpt/gpt-6.1-terra".into()),
        autonomy: Some("medium".into()),
        ..State::default()
    };
    s.save(&path);
    assert_eq!(State::load(&path), s);
    std::fs::write(&path, "{ not json").unwrap();
    assert_eq!(State::load(&path), State::default());
    std::fs::write(&path, r#"{"model":"x/y"}"#).unwrap();
    assert!(
        State::load(&path).remember,
        "missing keys fall back to defaults"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn remember_off_hides_the_saved_choices() {
    let mut s = State {
        effort: Some("high".into()),
        ..State::default()
    };
    assert_eq!(s.last(&s.effort), Some("high"));
    s.remember = false;
    assert_eq!(s.last(&s.effort), None);
}
