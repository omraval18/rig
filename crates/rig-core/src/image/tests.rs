use super::*;

#[test]
fn base64_matches_rfc4648_vectors() {
    for (input, want) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64(input.as_bytes()), want);
    }
}

#[test]
fn sniffing_beats_the_file_name() {
    assert_eq!(sniff(&[0x89, b'P', b'N', b'G', 0, 0]), Some("image/png"));
    assert_eq!(sniff(b"hello"), None);
    assert_eq!(mime_for_path(Path::new("a/Shot.PNG")), Some("image/png"));
    assert_eq!(mime_for_path(Path::new("notes.txt")), None);
}
