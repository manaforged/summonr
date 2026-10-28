use super::*;

#[test]
fn writing_over_a_file_reports_a_summary_not_the_whole_diff() {
    let (_dir, tools) = workspace(&[("a.txt", "old one\nold two\nold three\n")]);
    let text = run(
        &tools,
        "write",
        serde_json::json!({"path": "a.txt", "content": "new\n"}),
    );
    assert!(text.contains("replaced"), "{text}");
    assert!(!text.contains("-old one"), "{text}");
}

#[test]
fn two_edits_on_one_line_both_apply() {
    let (dir, tools) = workspace(&[("a.py", "a = x.astype(n.int) + y.astype(n.bool)\n")]);
    let text = run(
        &tools,
        "edit",
        serde_json::json!({"path": "a.py", "edits": [
            {"old": ".astype(n.int)", "new": ".astype(int)"},
            {"old": ".astype(n.bool)", "new": ".astype(bool)"}
        ]}),
    );
    assert!(!text.contains("overlap"), "{text}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.py")).unwrap(),
        "a = x.astype(int) + y.astype(bool)\n"
    );
}

#[test]
fn an_edit_with_unknown_keys_names_them() {
    let (_dir, tools) = workspace(&[("a.txt", "one\n")]);
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "edit".to_string(),
            arguments: serde_json::json!({"path": "a.txt", "old_tags": "1:ab", "new": "two"})
                .to_string(),
        },
        continuation: None,
    };
    let outcome = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call));
    let Err(Error::Tool(message)) = outcome else {
        panic!("expected a tool error");
    };
    assert!(message.contains("old_tags"), "{message}");
}

#[test]
fn a_large_edit_returns_a_short_diff() {
    let before: String = (0..200).map(|n| format!("old {n}\n")).collect();
    let after: String = (0..200).map(|n| format!("new {n}\n")).collect();
    let (_dir, tools) = workspace(&[("a.txt", &before)]);
    let text = run(
        &tools,
        "edit",
        serde_json::json!({"path": "a.txt", "old": before, "new": after}),
    );
    assert!(text.len() < 200, "{text}");
    assert!(text.contains("lines 1-200"), "{text}");
}

#[test]
fn checked_file_batches_preview_validate_all_files_and_then_apply() {
    let (dir, tools) = workspace(&[("a.txt", "old old\n"), ("b.txt", "old\n")]);
    let mut args = serde_json::json!({"files": [
        {"path": "a.txt", "old": "old", "new": "new", "expected_matches": 2},
        {"path": "b.txt", "old": "old", "new": "new", "expected_matches": 1}
    ], "preview": true});
    let text = run(&tools, "edit", args.clone());
    assert!(text.contains("Preview only"), "{text}");
    assert!(text.contains("new new"), "{text}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "old old\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "old\n"
    );
    args["preview"] = serde_json::json!(false);
    args["files"][1]["expected_matches"] = serde_json::json!(2);
    let text = outcome_of(&tools, "edit", args.clone());
    assert!(text.contains("expected 2 exact matches"), "{text}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "old old\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "old\n"
    );
    args["files"][1]["expected_matches"] = serde_json::json!(1);
    let text = run(&tools, "edit", args);
    assert!(
        text.contains("edited a.txt") && text.contains("edited b.txt"),
        "{text}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "new new\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "new\n"
    );
}

#[test]
fn patch_moves_preserve_edits_and_leave_the_git_index_unchanged() {
    let (dir, tools) = workspace(&[("a.txt", "one\ntwo\n")]);
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap()
    };
    assert!(git(&["init", "-q"]).status.success());
    assert!(git(&["add", "a.txt"]).status.success());
    let index = git(&["ls-files", "--stage"]).stdout;
    let text = run(
        &tools,
        "apply_patch",
        serde_json::json!({"patch": "*** Begin Patch\n*** Update File: a.txt\n*** Move to: nested/b.txt\n@@\n one\n-two\n+three\n*** End Patch\n"}),
    );
    assert!(text.contains("moved a.txt"), "{text}");
    assert!(!dir.path().join("a.txt").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("nested/b.txt")).unwrap(),
        "one\nthree\n"
    );
    assert_eq!(git(&["ls-files", "--stage"]).stdout, index);
    let text = run(
        &tools,
        "apply_patch",
        serde_json::json!({"patch": "*** Begin Patch\n*** Update File: nested/b.txt\n*** Move to: c.txt\n*** End Patch\n"}),
    );
    assert!(text.contains("moved nested/b.txt"), "{text}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("c.txt")).unwrap(),
        "one\nthree\n"
    );
}

#[test]
fn patch_move_destinations_are_checked_before_any_changes() {
    let (dir, tools) = workspace(&[("a.txt", "one\n"), ("b.txt", "two\n")]);
    let text = outcome_of(
        &tools,
        "apply_patch",
        serde_json::json!({"patch": "*** Begin Patch\n*** Update File: a.txt\n*** Move to: b.txt\n@@\n-one\n+three\n*** End Patch\n"}),
    );
    assert!(text.contains("already exists"), "{text}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "two\n"
    );
    let text = outcome_of(
        &tools,
        "apply_patch",
        serde_json::json!({"patch": "*** Begin Patch\n*** Update File: a.txt\n*** Move to: c.txt\n*** Update File: b.txt\n*** Move to: c.txt\n*** End Patch\n"}),
    );
    assert!(text.contains("multiple changes"), "{text}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "two\n"
    );
    assert!(!dir.path().join("c.txt").exists());
}
