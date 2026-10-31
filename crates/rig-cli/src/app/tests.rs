use super::*;
use crate::state::State;
use rig_core::permission::Autonomy;
use rig_core::provider::Effort;

/// An app whose ChatGPT login exists, so `chatgpt/*` counts as usable.
fn app(state: State, cfg_model: Option<&str>) -> App {
    let root = std::env::temp_dir().join(format!(
        "rig-app-{}-{}",
        std::process::id(),
        state.model.clone().unwrap_or_default().len()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("auth.json"), "{}").unwrap();
    let cfg = Config {
        model: cfg_model.map(String::from),
        ..Config::default()
    };
    App {
        paths: Paths::for_profile(root, crate::profile::DEFAULT.into(), None),
        cfg,
        keys: crate::keys::Keys::default(),
        cwd: std::env::temp_dir(),
        state,
    }
}

#[test]
fn what_you_used_last_beats_the_config_file_and_a_flag_beats_both() {
    let state = State {
        model: Some("chatgpt/gpt-6.1-terra".into()),
        ..State::default()
    };
    let a = app(state, Some("chatgpt/gpt-6-sol"));
    assert_eq!(a.resolve_model(None).unwrap().1, "gpt-6.1-terra");
    assert_eq!(
        a.resolve_model(Some("chatgpt/gpt-6-luna")).unwrap().1,
        "gpt-6-luna"
    );
}

#[test]
fn turning_remembering_off_falls_back_to_the_config_file() {
    let state = State {
        model: Some("chatgpt/gpt-6.1-terra".into()),
        remember: false,
        ..State::default()
    };
    let a = app(state, Some("chatgpt/gpt-6-sol"));
    assert_eq!(a.resolve_model(None).unwrap().1, "gpt-6-sol");
}

#[test]
fn a_saved_model_whose_provider_lost_its_login_is_skipped() {
    let state = State {
        model: Some("anthropic/claude-sonnet-5-5".into()),
        ..State::default()
    };
    let a = app(state, Some("chatgpt/gpt-6-sol"));
    assert_eq!(a.resolve_model(None).unwrap().1, "gpt-6-sol");
}

#[test]
fn scripts_never_inherit_a_saved_permission_level() {
    let state = State {
        autonomy: Some("high".into()),
        effort: Some("high".into()),
        ..State::default()
    };
    let a = app(state, None);
    assert_eq!(
        a.autonomy(None, true).unwrap(),
        Autonomy::High,
        "an interactive chat starts where you left off"
    );
    assert_eq!(
        a.autonomy(None, false).unwrap(),
        Autonomy::Low,
        "rig ask stays at the default"
    );
    assert_eq!(
        a.autonomy(Some("ask"), true).unwrap(),
        Autonomy::Ask,
        "a flag always wins"
    );
    assert_eq!(a.effort(None).unwrap(), Some(Effort::High));
}

#[test]
fn a_key_saved_in_the_profile_counts_and_beats_the_environment() {
    let mut a = app(State::default(), None);
    let spec = a.provider_spec("anthropic").unwrap();
    let before = a.credential(&spec);
    assert!(matches!(
        before,
        crate::app::Credential::Missing(_) | crate::app::Credential::Env(_)
    ));
    a.keys.set("anthropic", "sk-ant-test");
    assert_eq!(a.credential(&spec), crate::app::Credential::Saved);
    assert!(a.has_credentials(&spec));
    assert!(a.build_provider(&spec, "claude-sonnet-5-5").is_ok());
}

#[test]
fn a_named_profile_layers_its_config_between_the_shared_and_the_project_one() {
    let base = std::path::PathBuf::from("/r");
    let p = Paths::for_profile(base.clone(), "work".into(), None);
    assert_eq!(p.root, base.join("profiles/work"));
    assert_eq!(p.chatgpt_auth_file(), base.join("profiles/work/auth.json"));
    assert_eq!(
        p.config_files(std::path::Path::new("/w")),
        [
            base.join("config.toml"),
            base.join("profiles/work/config.toml"),
            std::path::PathBuf::from("/w/.rig/config.toml"),
        ]
    );
    let d = Paths::for_profile(base.clone(), crate::profile::DEFAULT.into(), None);
    assert_eq!(d.root, base, "the default profile is ~/.rig itself");
    assert_eq!(d.config_files(std::path::Path::new("/w")).len(), 2);
}
