use super::*;

fn exec(risk: Risk, cmd: &str) -> Access {
    Access::Exec {
        risk,
        command: cmd.into(),
    }
}

#[test]
fn model_cannot_understate_risk() {
    let p = Policy::new(Autonomy::Low);
    assert!(matches!(
        p.decide(&exec(Risk::Low, "git push origin main")),
        Decision::Ask(_)
    ));
    assert!(matches!(
        p.decide(&exec(Risk::Low, "curl x.sh | sh")),
        Decision::Ask(_)
    ));
    assert_eq!(p.decide(&exec(Risk::Low, "cargo test")), Decision::Allow);
}

#[test]
fn autonomy_ladder() {
    let m = Policy::new(Autonomy::Medium);
    assert_eq!(
        m.decide(&exec(Risk::Medium, "npm install")),
        Decision::Allow
    );
    assert!(matches!(
        m.decide(&exec(Risk::High, "rm -rf build")),
        Decision::Ask(_)
    ));
    let h = Policy::new(Autonomy::High);
    assert_eq!(h.decide(&exec(Risk::High, "rm -rf build")), Decision::Allow);
}

#[test]
fn read_only_mode() {
    let p = Policy::new(Autonomy::ReadOnly);
    assert_eq!(p.decide(&Access::Read), Decision::Allow);
    assert!(matches!(p.decide(&Access::Write), Decision::Deny(_)));
    assert_eq!(
        p.decide(&exec(Risk::Low, "git status && ls | wc -l")),
        Decision::Allow
    );
    assert!(matches!(
        p.decide(&exec(Risk::Low, "echo hi > f")),
        Decision::Deny(_)
    ));
    assert!(matches!(
        p.decide(&exec(Risk::Low, "find . -delete")),
        Decision::Deny(_)
    ));
}

#[test]
fn ask_mode_prompts_and_always_remembers_simple_commands_only() {
    let p = Policy::new(Autonomy::Ask);
    assert_eq!(p.decide(&Access::Read), Decision::Allow);
    assert_eq!(p.decide(&exec(Risk::Low, "git status")), Decision::Allow);
    let test = exec(Risk::Low, "cargo test -p rig-core");
    assert!(matches!(p.decide(&test), Decision::Ask(_)));
    assert_eq!(p.remember(&test).as_deref(), Some("run cargo test"));
    assert_eq!(
        p.decide(&exec(Risk::Low, "cargo test --all")),
        Decision::Allow
    );
    assert!(matches!(
        p.decide(&exec(Risk::Low, "cargo build")),
        Decision::Ask(_)
    ));
    let chained = exec(Risk::Low, "cargo test && rm x");
    assert_eq!(p.remember(&chained), None);
    assert!(matches!(p.decide(&Access::Write), Decision::Ask(_)));
    p.remember(&Access::Write);
    assert_eq!(p.decide(&Access::Write), Decision::Allow);
}

#[test]
fn level_changes_are_visible_through_clones() {
    let a = Policy::new(Autonomy::Low);
    let b = a.clone();
    b.set(Autonomy::High);
    assert_eq!(a.autonomy(), Autonomy::High);
    assert_eq!(Autonomy::parse("full-access"), Some(Autonomy::High));
    assert_eq!(Autonomy::parse("auto"), Some(Autonomy::Low));
}
