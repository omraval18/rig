use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn fake_server() -> ServerSpec {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/testdata/fake_server.py");
    ServerSpec {
        command: "python3".into(),
        args: vec!["-I".into(), script.display().to_string()],
        env: BTreeMap::new(),
        enabled: true,
    }
}

fn hub() -> Hub {
    Hub::new(&BTreeMap::from([("notes".to_string(), fake_server())]))
}

#[test]
fn nothing_starts_until_a_tool_is_searched_or_called() {
    let hub = hub();
    assert_eq!(hub.names(), ["notes"]);
    assert_eq!(hub.status(), ["notes: not started (starts on first use)"]);
    assert!(catalog(&hub).unwrap().contains("notes"));
}

#[test]
fn search_starts_the_servers_and_finds_tools_by_words() {
    let hub = hub();
    let hits = hub.search("repeat", 5);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].tool.name, "echo");
    assert!(hits[0].tool.read_only);
    assert_eq!(hub.status(), ["notes: connected, 2 tools"]);
    assert!(hub.search("nothing like this", 5).is_empty());
}

#[test]
fn a_tool_runs_and_its_read_only_hint_is_kept() {
    let hub = hub();
    assert_eq!(hub.lookup("notes", "echo"), Some(true));
    assert_eq!(hub.lookup("notes", "write_note"), Some(false));
    let (text, is_error) = hub
        .call("notes", "echo", serde_json::json!({"text": "hi there"}))
        .unwrap();
    assert_eq!((text.as_str(), is_error), ("hi there", false));
}

#[test]
fn unknown_servers_and_tools_are_errors_not_panics() {
    let hub = hub();
    assert!(hub.call("nope", "echo", serde_json::json!({})).is_err());
    assert!(hub.call("notes", "missing", serde_json::json!({})).is_err());
    assert_eq!(hub.lookup("notes", "missing"), None);
}

#[test]
fn a_server_that_cannot_start_is_reported_not_retried() {
    let spec = ServerSpec {
        command: "/definitely/not/a/program".into(),
        ..ServerSpec::default()
    };
    let hub = Hub::new(&BTreeMap::from([("bad".to_string(), spec)]));
    assert!(hub.search("anything", 3).is_empty());
    assert!(hub.status()[0].starts_with("bad: failed: cannot start"));
    assert!(hub.call("bad", "x", serde_json::json!({})).is_err());
}

#[test]
fn disabled_servers_are_left_out() {
    let mut spec = fake_server();
    spec.enabled = false;
    let hub = Hub::new(&BTreeMap::from([("off".to_string(), spec)]));
    assert!(hub.is_empty());
    assert!(tools(std::sync::Arc::new(hub)).is_empty());
}
