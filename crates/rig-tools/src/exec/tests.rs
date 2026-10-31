use super::*;

fn run(cmd: &str, timeout: u64) -> ToolOutput {
    let ctx = ToolCtx::new(std::env::temp_dir(), "t".into());
    RunCommand.run(
        &json!({"purpose": "t", "command": cmd, "risk": "low", "timeout_secs": timeout}),
        &ctx,
    )
}

#[test]
fn captures_both_streams_and_exit_code() {
    let o = run("echo out; echo err >&2; exit 3", 5);
    assert!(o.text.contains("out") && o.text.contains("err"));
    assert!(o.text.contains("exited with code 3") && o.is_error);
}

#[test]
fn timeout_kills_the_process_group() {
    let t = Instant::now();
    let o = run("sleep 30 & sleep 30", 1);
    assert!(o.text.contains("timed out"));
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
fn cancel_kills_a_running_command_quickly() {
    let ctx = ToolCtx::new(std::env::temp_dir(), "t".into());
    let shared = ctx.shared.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        shared
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let t = Instant::now();
    let o = RunCommand.run(
        &json!({"purpose": "t", "command": "sleep 30", "risk": "low"}),
        &ctx,
    );
    assert!(o.text.contains("cancelled") && o.is_error);
    assert!(t.elapsed() < Duration::from_secs(3));
}

#[test]
fn long_output_keeps_head_and_tail() {
    let o = run("seq 1 100000", 10);
    assert!(o.text.starts_with("1\n") && o.text.contains("bytes cut") && o.text.contains("100000"));
}

fn sum(text: &str, is_error: bool) -> Option<String> {
    RunCommand.summarize(
        &Value::Null,
        &ToolOutput {
            text: text.into(),
            is_error,
        },
    )
}

#[test]
fn failure_shows_the_exit_code_and_the_line_that_matters_not_a_separator() {
    let out = "test_add ... FAIL\n\n----------------------------------------------------------------------\nRan 2 tests\n\nFAILED (failures=1)\n\n[Process exited with code 1]";
    assert_eq!(
        sum(out, true).as_deref(),
        Some("exit 1 · FAILED (failures=1)")
    );
}

#[test]
fn success_shows_the_last_line_of_output() {
    assert_eq!(
        sum(
            "collected 2\n2 passed in 0.01s\n\n[Process exited with code 0]",
            false
        )
        .as_deref(),
        Some("2 passed in 0.01s")
    );
    assert_eq!(
        sum("\n\n[Process exited with code 0]", false),
        None,
        "silent commands stay silent"
    );
}
