use super::*;

#[test]
fn blocks_local_and_private_targets() {
    for u in [
        "http://localhost:3000/x",
        "http://127.0.0.1/",
        "http://10.1.2.3/",
        "http://192.168.1.1/",
        "http://172.20.0.1/",
        "http://169.254.169.254/latest",
        "http://svc.internal/",
        "http://user:pw@example.com/",
        "ftp://example.com/",
        "http://[::1]/",
        "http://intranet/",
    ] {
        assert!(check_url(u).is_err(), "{u} should be blocked");
    }
    assert!(check_url("https://docs.rs/serde").is_ok());
    assert!(check_url("http://172.32.0.1/").is_ok());
}

#[test]
fn html_becomes_readable_text() {
    let t = html_to_text(
        "<html><head><style>p{}</style></head><body><h1>Title</h1><p>Hello &amp; <b>world</b></p><script>x()</script><ul><li>a</li><li>b</li></ul></body></html>",
    );
    assert!(
        t.contains("# Title")
            && t.contains("Hello & world")
            && t.contains("- a")
            && !t.contains("x()")
            && !t.contains("p{}")
    );
}

#[test]
fn relative_redirects() {
    assert_eq!(absolutize("https://a.com/x/y", "/z"), "https://a.com/z");
    assert_eq!(
        absolutize("https://a.com/x/y", "https://b.com/"),
        "https://b.com/"
    );
}
