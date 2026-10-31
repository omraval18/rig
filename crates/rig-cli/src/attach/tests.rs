use super::*;

#[test]
fn finds_image_tokens_and_ignores_lookalikes() {
    let c: Vec<char> = "see [Image #1] and [Image #12]! [Image #] [image #2] [Image #3"
        .chars()
        .collect();
    let t = image_tokens(&c);
    assert_eq!(t.iter().map(|x| x.2).collect::<Vec<_>>(), [1, 12]);
    assert_eq!(c[t[0].0..t[0].1].iter().collect::<String>(), "[Image #1]");
}

#[test]
fn dropped_paths_may_be_quoted_escaped_or_urls() {
    let dir = std::env::temp_dir().join(format!("rig-drop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("Screen Shot.png");
    std::fs::write(&p, [0x89, b'P', b'N', b'G']).unwrap();
    let shown = p.display().to_string();
    assert_eq!(
        dropped_image_path(&shown.replace(' ', "\\ ")),
        Some(p.clone())
    );
    assert_eq!(dropped_image_path(&format!("'{shown}'")), Some(p.clone()));
    assert_eq!(
        dropped_image_path(&format!("file://{}", shown.replace(' ', "\\ "))),
        Some(p)
    );
    assert_eq!(dropped_image_path("/definitely/not/here.png"), None);
    assert_eq!(dropped_image_path("notes.txt"), None);
    std::fs::remove_dir_all(dir).ok();
}
