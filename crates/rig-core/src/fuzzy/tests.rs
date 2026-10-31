use super::*;

#[test]
fn exact_beats_prefix_beats_word_beats_scattered() {
    let exact = rank_label("main", "main").unwrap().score;
    let prefix = rank_label("main", "main_loop").unwrap().score;
    let word = rank_label("loop", "main_loop").unwrap().score;
    let scattered = rank_label("mnlp", "main_loop").unwrap().score;
    assert!(
        exact > prefix && prefix > word && word > scattered,
        "{exact} {prefix} {word} {scattered}"
    );
}

#[test]
fn missing_letter_does_not_match() {
    assert!(subsequence("xyz", "main.rs").is_none());
    assert!(rank_label("zz", "main").is_none());
}

#[test]
fn path_ranking_prefers_file_name_then_shallow() {
    let a = rank_path("main", "src/main.rs").unwrap().score;
    let b = rank_path("main", "src/deep/er/main.rs").unwrap().score;
    let c = rank_path("main", "src/domain/mapper.rs").unwrap().score; // scattered across name
    assert!(a > b && b > c, "{a} {b} {c}");
}

#[test]
fn positions_point_into_the_file_name() {
    let m = rank_path("lib", "crates/core/src/lib.rs").unwrap();
    assert_eq!(m.positions, vec![16, 17, 18]);
}

#[test]
fn slash_queries_match_whole_path_prefix() {
    let m = rank_path("src/ma", "src/main.rs").unwrap();
    assert_eq!(m.positions.len(), 6);
    assert!(rank_path("src/ma", "tests/main.rs").is_none());
}
