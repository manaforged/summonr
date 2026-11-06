use super::*;

fn search(tools: &ToolRegistry, arguments: serde_json::Value) -> String {
    run(tools, "search", arguments)
}

#[test]
fn name_words_and_bare_globs_find_files_without_wildcards() {
    let (_dir, tools) = workspace(&[
        ("Q3-Report-FINAL-v2.txt", "x\n"),
        ("q2-report.txt", "x\n"),
        ("README.md", "x\n"),
    ]);
    let named = search(&tools, serde_json::json!({"name": "report q3"}));
    assert!(named.contains("Q3-Report-FINAL-v2.txt"), "{named}");
    assert!(!named.contains("q2-report"), "{named}");
    let none = search(&tools, serde_json::json!({"name": "q4"}));
    assert!(
        none.starts_with("no files match (3 files searched)"),
        "{none}"
    );
    let bare = search(&tools, serde_json::json!({"glob": "readme"}));
    assert!(bare.contains("README.md"), "{bare}");
}

#[test]
fn word_and_pattern_lists_shape_the_match() {
    let (_dir, tools) = workspace(&[("a.rs", "let id = 1;\nlet idle = 2;\nfn other() {}\n")]);
    let word = search(&tools, serde_json::json!({"pattern": "id", "word": true}));
    assert!(word.contains("a.rs:1:"), "{word}");
    assert!(!word.contains("a.rs:2:"), "{word}");
    let either = search(&tools, serde_json::json!({"pattern": ["idle", "other"]}));
    assert!(
        either.contains("a.rs:2:") && either.contains("a.rs:3:"),
        "{either}"
    );
}

#[test]
fn newer_group_and_offset_answer_without_the_shell() {
    let (dir, tools) = workspace(&[("old.txt", "x\n"), ("new.txt", "x\n")]);
    std::fs::create_dir_all(dir.path().join("lib/deep")).unwrap();
    for name in ["lib/a.dylib", "lib/b.dylib", "lib/deep/c.dylib"] {
        std::fs::write(dir.path().join(name), "x").unwrap();
    }
    std::fs::File::options()
        .write(true)
        .open(dir.path().join("old.txt"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(5 * 86_400))
        .unwrap();
    let recent = search(&tools, serde_json::json!({"glob": "*.txt", "newer": "2d"}));
    assert!(
        recent.contains("new.txt") && !recent.contains("old.txt"),
        "{recent}"
    );
    let grouped = search(
        &tools,
        serde_json::json!({"glob": "*.dylib", "group": "folder"}),
    );
    assert!(grouped.starts_with("3 files in 2 folders"), "{grouped}");
    assert!(grouped.contains("     1  lib/deep"), "{grouped}");
    assert!(grouped.contains("     2  lib"), "{grouped}");
    let first = search(&tools, serde_json::json!({"glob": "*.dylib", "limit": 2}));
    assert!(first.contains("offset 2 shows the next files"), "{first}");
    let rest = search(
        &tools,
        serde_json::json!({"glob": "*.dylib", "limit": 2, "offset": 2}),
    );
    assert!(rest.contains("showing 3–3"), "{rest}");
    assert_eq!(rest.lines().count(), 2, "{rest}");
}

#[test]
fn no_ignore_reaches_build_folders_and_overlapping_paths_keep_depth() {
    let (dir, tools) = workspace(&[]);
    std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    std::fs::write(dir.path().join("node_modules/pkg/index.js"), "needle\n").unwrap();
    std::fs::create_dir_all(dir.path().join("src/deep")).unwrap();
    std::fs::write(dir.path().join("src/deep/x.rs"), "needle\n").unwrap();
    let skipped = search(&tools, serde_json::json!({"pattern": "needle"}));
    assert!(!skipped.contains("node_modules"), "{skipped}");
    let all = search(
        &tools,
        serde_json::json!({"pattern": "needle", "no_ignore": true}),
    );
    assert!(all.contains("node_modules/pkg/index.js"), "{all}");
    let both = search(
        &tools,
        serde_json::json!({"pattern": "needle", "paths": ["src", "src/deep"], "depth": 1}),
    );
    assert!(both.contains("src/deep/x.rs"), "{both}");
}

#[test]
fn a_tag_from_a_long_line_edits_that_line() {
    let long = format!("let value = \"{}\";", "x".repeat(600));
    let (dir, tools) = workspace(&[("a.rs", &format!("{long}\nend\n"))]);
    let hits = search(
        &tools,
        serde_json::json!({"pattern": "let value", "tags": true}),
    );
    let tag = hits
        .lines()
        .find_map(|line| line.strip_prefix("a.rs:"))
        .and_then(|rest| rest.split('|').next())
        .unwrap_or_else(|| panic!("no tag in {hits}"))
        .to_string();
    run(
        &tools,
        "edit",
        serde_json::json!({"path": "a.rs", "tag": tag, "new": "let value = 1;"}),
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.rs")).unwrap(),
        "let value = 1;\nend\n"
    );
}

#[test]
fn the_schema_stays_small_and_old_argument_names_still_work() {
    let (dir, tools) = workspace(&[("a.rs", "needle\n"), ("b.rs", "needle\n")]);
    std::fs::create_dir_all(dir.path().join("lib")).unwrap();
    std::fs::write(dir.path().join("lib/c.rs"), "needle\n").unwrap();
    let definition = tools
        .definitions()
        .into_iter()
        .find(|tool| {
            crate::tools::Builtin::live(&tool.function.name) == Some(crate::tools::Builtin::Search)
        })
        .unwrap();
    let properties = definition.function.parameters["properties"]
        .as_object()
        .unwrap()
        .len();
    assert!(properties <= 15, "{properties} search arguments");
    let files = search(
        &tools,
        serde_json::json!({"pattern": "needle", "files": true}),
    );
    let listed = search(
        &tools,
        serde_json::json!({"pattern": "needle", "output": "files"}),
    );
    assert_eq!(files, listed);
    let folders = search(
        &tools,
        serde_json::json!({"glob": "*.rs", "output": "folders"}),
    );
    assert!(folders.starts_with("3 files in 2 folders"), "{folders}");
    let both = search(
        &tools,
        serde_json::json!({"pattern": "needle", "path": ["lib", "a.rs"]}),
    );
    assert!(
        both.contains("lib/c.rs") && both.contains("a.rs:1:"),
        "{both}"
    );
    assert!(!both.contains("b.rs"), "{both}");
    let past = search(&tools, serde_json::json!({"glob": "*.rs", "offset": 9}));
    assert_eq!(past, "no entries at offset 9: 3 files in all");
}

#[test]
fn a_flood_of_matches_is_cut_at_the_output_cap() {
    let block = format!(
        "needle here\n{}",
        "filler text that only pads the context around each match\n".repeat(20)
    );
    let text = block.repeat(40);
    let names: Vec<String> = (0..10).map(|at| format!("f{at}.txt")).collect();
    let files: Vec<(&str, &str)> = names
        .iter()
        .map(|name| (name.as_str(), text.as_str()))
        .collect();
    let (_dir, tools) = workspace(&files);
    let hits = search(
        &tools,
        serde_json::json!({"pattern": "needle", "context": 10, "limit": 200}),
    );
    assert!(hits.len() <= 50 * 1024 + 100, "{}", hits.len());
    assert!(
        hits.ends_with("narrow path]"),
        "{}",
        &hits[hits.len() - 80..]
    );
}
