use super::*;

#[test]
fn aliases_resolve() {
    assert_eq!(lookup("/clear").unwrap().cmd, Cmd::New);
    assert_eq!(lookup("exit").unwrap().cmd, Cmd::Quit);
    assert_eq!(lookup("/SETUP").unwrap().cmd, Cmd::Provider);
    assert!(lookup("/nope").is_none());
}

#[test]
fn prefix_ranks_first() {
    let names: Vec<_> = complete("mo").iter().map(|c| c.spec.name).collect();
    assert_eq!(names[0], "model");
    let names: Vec<_> = complete("co").iter().map(|c| c.spec.name).collect();
    let mut top = names[..2].to_vec();
    top.sort();
    assert_eq!(
        top,
        ["compact", "copy"],
        "both prefix matches outrank everything else"
    );
}

#[test]
fn empty_query_lists_all_and_alias_matches() {
    assert_eq!(complete("").len(), COMMANDS.len());
    assert_eq!(complete("exi")[0].spec.cmd, Cmd::Quit);
}

#[test]
fn every_command_name_is_unique() {
    let mut names: Vec<_> = COMMANDS
        .iter()
        .flat_map(|s| std::iter::once(s.name).chain(s.aliases.iter().copied()))
        .collect();
    let n = names.len();
    names.sort();
    names.dedup();
    assert_eq!(n, names.len());
}
