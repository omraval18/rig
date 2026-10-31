use super::*;

fn run_in(dir: &std::path::Path, patch: &str) -> ToolOutput {
    let ctx = ToolCtx::new(dir.to_path_buf(), "t".into());
    EditFiles.run(&json!({ "patch": patch }), &ctx)
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rig-patch-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn add_update_move_delete() {
    let d = tmp("ops");
    fs::write(d.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    fs::write(d.join("gone.txt"), "x\n").unwrap();
    let patch = "*** Begin Patch\n*** Add File: new/b.txt\n+hello\n*** Update File: a.txt\n*** Move to: c.txt\n@@\n one\n-two\n+TWO\n three\n*** Delete File: gone.txt\n*** End Patch\n";
    let o = run_in(&d, patch);
    assert!(!o.is_error, "{}", o.text);
    assert_eq!(fs::read_to_string(d.join("new/b.txt")).unwrap(), "hello\n");
    assert_eq!(
        fs::read_to_string(d.join("c.txt")).unwrap(),
        "one\nTWO\nthree\n"
    );
    assert!(!d.join("a.txt").exists() && !d.join("gone.txt").exists());
}

#[test]
fn nothing_is_written_when_a_hunk_fails() {
    let d = tmp("atomic");
    fs::write(d.join("a.txt"), "one\n").unwrap();
    let patch = "*** Begin Patch\n*** Add File: made.txt\n+x\n*** Update File: a.txt\n@@\n-missing\n+y\n*** End Patch\n";
    let o = run_in(&d, patch);
    assert!(o.is_error);
    assert!(!d.join("made.txt").exists());
    assert_eq!(fs::read_to_string(d.join("a.txt")).unwrap(), "one\n");
}

#[test]
fn tolerates_indentation_drift_and_anchor() {
    let d = tmp("fuzzy");
    fs::write(
        d.join("f.rs"),
        "fn a() {\n    x();\n}\nfn b() {\n    y();\n}\n",
    )
    .unwrap();
    let patch =
        "*** Begin Patch\n*** Update File: f.rs\n@@ fn b() {\n-y();\n+z();\n*** End Patch\n";
    assert!(!run_in(&d, patch).is_error);
    assert_eq!(
        fs::read_to_string(d.join("f.rs")).unwrap(),
        "fn a() {\n    x();\n}\nfn b() {\nz();\n}\n"
    );
}

#[test]
fn undo_restores_edits_creations_and_deletions() {
    let d = tmp("undo");
    fs::write(d.join("a.txt"), "one\n").unwrap();
    fs::write(d.join("gone.txt"), "keep me\n").unwrap();
    let ctx = ToolCtx::new(d.clone(), "t".into());
    let patch = "*** Begin Patch\n*** Add File: new.txt\n+x\n*** Update File: a.txt\n@@\n-one\n+ONE\n*** Delete File: gone.txt\n*** End Patch\n";
    assert!(!EditFiles.run(&json!({ "patch": patch }), &ctx).is_error);
    let msg = crate::undo_last(&ctx.shared).unwrap();
    assert!(msg.contains("3 file"), "{msg}");
    assert_eq!(fs::read_to_string(d.join("a.txt")).unwrap(), "one\n");
    assert_eq!(fs::read_to_string(d.join("gone.txt")).unwrap(), "keep me\n");
    assert!(!d.join("new.txt").exists());
    assert!(
        crate::undo_last(&ctx.shared).is_none(),
        "nothing left to undo"
    );
}

#[test]
fn rejects_garbage() {
    assert!(parse("hello").is_err());
    assert!(parse("*** Begin Patch\n*** End Patch\n").is_err());
}
