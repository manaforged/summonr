use super::edit::{Patch, apply_patches};

#[test]
fn a_patch_past_the_last_newline_does_not_panic() {
    let original = "a\nb\n";
    let (text, _) = apply_patches(
        original,
        vec![Patch {
            start: 0,
            end: 8,
            anchor_start: 0,
            anchor_end: 2,
            replacement: vec!["x".to_string()],
            relaxed: false,
        }],
    )
    .unwrap();
    assert_eq!(text, "x\n");
}

#[test]
fn an_edit_names_each_changed_region_and_nothing_it_did_not_touch() {
    let before: String = (1..400).map(|n| format!("line {n}\n")).collect();
    let after = before
        .replace("line 100\n", "alpha\nalpha extra\n")
        .replace("line 250\n", "beta\n");
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let (spans, _) = super::spans::changed_and_patch(&old, &new);
    assert_eq!(
        super::spans::summary(old.len(), new.len(), &spans),
        "changed lines 100-101, line 251 (+1 line, 400 lines now)"
    );
}

#[test]
fn a_display_patch_keeps_three_lines_of_context_around_each_change() {
    let before: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    let after = before.replace("line 10\n", "ten\n");
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    assert_eq!(
        super::spans::patch(&old, &new).unwrap(),
        "@@ -7,7 +7,7 @@\n line 7\n line 8\n line 9\n-line 10\n+ten\n line 11\n line 12\n line 13"
    );
}

#[test]
fn an_edit_hands_the_view_a_patch_and_the_model_a_summary() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.py"), "one\ntwo\nthree\n").unwrap();
    let tools = crate::tools::ToolRegistry::with_policy_and_search(
        dir.path(),
        crate::policy::Policy::builtin(),
        None,
    )
    .unwrap();
    let call = crate::provider::ToolCall {
        id: "t1".to_string(),
        kind: crate::provider::ToolKind::Function,
        function: crate::provider::ToolFunction {
            checked: Default::default(),
            name: "edit".to_string(),
            arguments: serde_json::json!({"path": "a.py", "old": "two", "new": "2"}).to_string(),
        },
        continuation: None,
    };
    let output = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call))
        .unwrap();
    assert_eq!(
        output.patch.as_deref(),
        Some("@@ -1,3 +1,3 @@\n one\n-two\n+2\n three")
    );
    assert!(!output.text.contains("@@"), "{}", output.text);
}

#[test]
fn a_display_patch_is_skipped_past_the_line_limit() {
    let lines: Vec<String> = (0..=crate::limits::limits().display_patch_lines)
        .map(|n| format!("line {n}"))
        .collect();
    let new: Vec<&str> = lines.iter().map(String::as_str).collect();
    assert_eq!(super::spans::patch(&[], &new), None);
    assert!(super::spans::patch(&[], &new[..10]).is_some());
}
