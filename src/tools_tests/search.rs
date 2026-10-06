use super::*;

#[test]
fn a_literal_search_matches_regex_characters_and_reports_their_column() {
    let (_dir, tools) = workspace(&[("a.txt", "let x = foo(bar);\naxb then a.b here\n")]);
    let hits = run(
        &tools,
        "read",
        serde_json::json!({"pattern": "foo(", "literal": true}),
    );
    assert!(hits.contains("a.txt:1:9:let x = foo(bar);"), "{hits}");
    let hits = run(
        &tools,
        "read",
        serde_json::json!({"pattern": "a.b", "literal": true}),
    );
    assert!(hits.contains("a.txt:2:10:axb then a.b here"), "{hits}");
}

#[test]
fn an_end_anchored_search_reports_the_utf16_column_of_the_match() {
    let (_dir, tools) = workspace(&[("a.txt", "😀 call(x)\n")]);
    let hits = run(&tools, "read", serde_json::json!({"pattern": "\\)$"}));
    assert!(hits.contains("a.txt:1:10:😀 call(x)"), "{hits}");
}

#[test]
fn the_tag_in_a_tagged_search_hit_edits_that_line() {
    let (dir, tools) = workspace(&[("a.txt", "one\ntwo\n")]);
    let hits = run(
        &tools,
        "read",
        serde_json::json!({"pattern": "two", "tags": true}),
    );
    let hit = hits
        .lines()
        .find_map(|line| line.strip_prefix("a.txt:"))
        .unwrap_or_else(|| panic!("no hit in {hits}"));
    let tag = hit.split('|').next().unwrap();
    run(
        &tools,
        "edit",
        serde_json::json!({"path": "a.txt", "tag": tag, "new": "TWO"}),
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\nTWO\n"
    );
}

#[test]
fn a_content_search_lists_the_most_recently_modified_file_first() {
    let (dir, tools) = workspace(&[("a.txt", "hit\n"), ("b.txt", "hit\n")]);
    std::fs::File::options()
        .write(true)
        .open(dir.path().join("a.txt"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    let hits = run(
        &tools,
        "read",
        serde_json::json!({"pattern": "hit", "limit": 1}),
    );
    assert!(hits.contains("b.txt:1:1:hit"), "{hits}");
}

#[test]
fn a_search_runs_across_several_paths() {
    let (dir, tools) = workspace(&[
        ("a.rs", "fn retry() {}\n"),
        ("b.rs", "fn retry_twice() {}\n"),
    ]);
    std::fs::create_dir_all(dir.path().join("c")).unwrap();
    let text = run(
        &tools,
        "read",
        serde_json::json!({"op": "search", "pattern": "retry", "paths": ["a.rs", "b.rs"]}),
    );
    assert!(text.contains("a.rs") && text.contains("b.rs"), "{text}");
}

#[test]
fn a_search_over_several_paths_reports_a_missing_one_inline() {
    let (_dir, tools) = workspace(&[("a.txt", "needle here\n")]);
    let hits = run(
        &tools,
        "read",
        serde_json::json!({"pattern": "needle", "paths": ["a.txt", "missing.txt"]}),
    );
    assert!(hits.contains("a.txt:1:"), "{hits}");
    assert!(hits.contains("missing.txt: "), "{hits}");
}
