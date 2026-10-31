use super::*;

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(std::iter::once("rig").chain(args.iter().copied())).unwrap()
}

#[test]
fn a_permission_level_named_like_a_subcommand_is_still_a_value() {
    let c = parse(&["--auto", "ask"]);
    assert_eq!(
        (c.opts.auto.as_deref(), c.cmd.is_none(), c.prompt.len()),
        (Some("ask"), true, 0)
    );
    let c = parse(&["--auto", "ask", "ask", "hello"]);
    assert!(matches!(c.cmd, Some(Cmd::Ask { .. })) && c.opts.auto.as_deref() == Some("ask"));
}

#[test]
fn subcommands_and_a_first_message_coexist() {
    assert!(matches!(parse(&["ask", "hi"]).cmd, Some(Cmd::Ask { .. })));
    assert!(matches!(parse(&["models"]).cmd, Some(Cmd::Models)));
    let c = parse(&["--auto", "medium", "--", "-- fix the tests"]);
    assert!(c.cmd.is_none() && c.prompt == ["-- fix the tests"]);
    assert_eq!(
        parse(&["fix", "the", "tests"]).prompt,
        ["fix", "the", "tests"]
    );
}

#[test]
fn rigd_is_full_access_and_rig_keeps_the_default() {
    use std::ffi::OsStr;
    assert_eq!(default_autonomy(Some(OsStr::new("rigd"))), Some("high"));
    assert_eq!(
        default_autonomy(Some(OsStr::new("/home/me/.local/bin/rigd"))),
        Some("high")
    );
    assert_eq!(default_autonomy(Some(OsStr::new("rig"))), None);
    assert_eq!(default_autonomy(Some(OsStr::new("/usr/bin/rig"))), None);
    assert_eq!(default_autonomy(None), None);
}
