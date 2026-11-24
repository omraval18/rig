use super::*;
use std::io::{BufRead, BufReader, Write};

#[test]
fn urls_are_cut_to_the_api_root_and_must_be_safe() {
    let ok = |u: &str| clean_url(u).unwrap();
    assert_eq!(
        ok(" https://api.x.com/v1/chat/completions "),
        "https://api.x.com/v1"
    );
    assert_eq!(ok("https://api.x.com/v1/messages"), "https://api.x.com");
    assert_eq!(ok("http://localhost:8000/"), "http://localhost:8000");
    for bad in [
        "http://example.com",
        "http://localhost.evil.com",
        "http://localhost@evil.com",
        "https://",
        "ftp://x",
        "",
    ] {
        assert!(clean_url(bad).is_err(), "{bad}");
    }
}

#[test]
fn providers_are_named_after_their_host() {
    let free = |_: &str| false;
    assert_eq!(name_for("https://api.example.com/v1", free), "example");
    assert_eq!(name_for("http://localhost:8000/v1", free), "local-8000");
    assert_eq!(name_for("https://10.0.0.5:9/v1", free), "10-0-0-5");
    assert_eq!(
        name_for("https://api.openai.com/v1", |n| n == "openai"),
        "openai-2"
    );
}

/// Answers `/v1/models` with one model and everything else with 404.
fn server(requests: usize) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().take(requests) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            let (status, body) = if first.starts_with("GET /v1/models ") {
                ("200 OK", r#"{"data":[{"id":"m1"}]}"#)
            } else {
                ("404 Not Found", "{}")
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
}

#[test]
fn a_bare_url_is_completed_and_its_models_found() {
    let url = server(2);
    let (kind, base, models) = probe(&url, "").unwrap();
    assert_eq!((kind, base), (Kind::Chat, format!("{url}/v1")));
    assert_eq!(models[0].id, "m1");
    assert!(probe("http://127.0.0.1:1", "").is_err());
}
