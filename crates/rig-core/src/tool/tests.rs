use super::*;

#[test]
fn error_line_finds_the_complaint_not_the_tail() {
    assert_eq!(
        error_line("ls: /x: No such file or directory\n.:\n").as_deref(),
        Some("ls: /x: No such file or directory")
    );
    assert_eq!(
        error_line("collected 2\n1 passed\n").as_deref(),
        Some("1 passed")
    );
    assert_eq!(
        error_line("Traceback...\nValueError: bad\n\n").as_deref(),
        Some("ValueError: bad")
    );
    // curl exit 6: the complaint is on stderr, the last line is the -w format output.
    assert_eq!(
        error_line("curl: (6) Could not resolve host: stage.example.com\nAPI HTTP 000\n")
            .as_deref(),
        Some("curl: (6) Could not resolve host: stage.example.com")
    );
}

#[test]
fn last_meaningful_line_skips_rules_and_blanks() {
    assert_eq!(
        last_meaningful_line("a\nFAILED (failures=1)\n\n----------\n"),
        Some("FAILED (failures=1)".into())
    );
    assert_eq!(last_meaningful_line("\n ==== \n"), None);
}
