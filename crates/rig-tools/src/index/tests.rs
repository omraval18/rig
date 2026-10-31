use super::*;
use std::time::{Duration, Instant};

fn build() -> (PathBuf, Index) {
    let root = std::env::temp_dir().join(format!(
        "rig-index-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for f in [
        "src/main.rs",
        "src/lib.rs",
        "src/deep/er/main.rs",
        "docs/readme.md",
        "README.md",
        "node_modules/x/index.js",
        "target/out.bin",
        ".hidden/file",
    ] {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }
    std::fs::write(root.join(".gitignore"), "node_modules/\ntarget/\n").unwrap();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    let idx = Index::spawn(root.clone(), Some(root.join("src")));
    let t = Instant::now();
    while !idx.ready() && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    (root, idx)
}

#[test]
fn respects_gitignore_and_skips_dot_git() {
    let (root, idx) = build();
    let all: Vec<String> = idx.search("", 100).into_iter().map(|h| h.path).collect();
    assert!(all.contains(&"src/".to_string()) && all.contains(&"README.md".to_string()));
    let hits: Vec<String> = idx
        .search("index", 10)
        .into_iter()
        .map(|h| h.path)
        .collect();
    assert!(
        hits.is_empty(),
        "ignored files must not be suggested: {hits:?}"
    );
    assert!(
        idx.search("file", 10)
            .iter()
            .any(|h| h.path == ".hidden/file"),
        "dotfiles are indexed"
    );
    assert!(
        !idx.search("git", 50)
            .iter()
            .any(|h| h.path.starts_with(".git/"))
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn ranks_exact_name_before_deeper_and_scattered() {
    let (root, idx) = build();
    let paths: Vec<String> = idx.search("main", 10).into_iter().map(|h| h.path).collect();
    assert_eq!(paths[0], "src/main.rs");
    assert_eq!(paths[1], "src/deep/er/main.rs");
    let readme = idx.search("readme", 10);
    assert_eq!(readme.len(), 2);
    assert!(readme.iter().all(|h| !h.is_dir));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn folders_end_with_slash_and_path_queries_work() {
    let (root, idx) = build();
    let h = idx.search("src/m", 10);
    assert_eq!(h[0].path, "src/main.rs");
    assert!(idx.search("docs", 5)[0].is_dir && idx.search("docs", 5)[0].path == "docs/");
    let inside: Vec<String> = idx
        .search("docs/", 10)
        .into_iter()
        .map(|h| h.path)
        .collect();
    assert_eq!(
        inside,
        ["docs/readme.md"],
        "an opened folder lists its contents, not itself"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn tilde_dot_and_dotdot_browse_real_directories() {
    let (root, idx) = build();
    let home = idx.search("~", 20);
    assert!(
        home.iter().any(|h| h.path == "~/main.rs"),
        "~ is the (test) home dir: {home:?}"
    );
    let parent = idx.search("..", 5);
    assert!(parent.iter().all(|h| h.path.starts_with("../")));
    let here = idx.search(".", 50);
    assert!(here.iter().any(|h| h.path == "./src/" && h.is_dir));
    let filtered = idx.search("~/li", 5);
    assert_eq!(filtered[0].path, "~/lib.rs");
    assert_eq!(filtered[0].positions, vec![2, 3]);
    std::fs::remove_dir_all(root).ok();
}
