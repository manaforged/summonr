use std::path::PathBuf;

use super::*;
use crate::policy::Policy;

#[test]
fn empty_run_command_arguments_are_a_tool_error() {
    let error = parse_args("run_command", "").unwrap_err();
    let Error::Tool(message) = error else {
        panic!("{error}");
    };
    assert!(
        message.contains("run_command arguments are invalid"),
        "{message}"
    );
}

#[test]
fn an_unmounted_volume_is_not_created() {
    let dir = tempfile::tempdir().unwrap();
    let mut tools =
        ToolRegistry::with_policy_and_search(dir.path(), Policy::builtin(), None).unwrap();
    let path = PathBuf::from("/Volumes/SummonrNoSuchVolume/dev");
    let error = tools.allow_path(path.clone()).unwrap_err();
    assert!(matches!(error, Error::Config(_)), "{error}");
    assert!(!path.exists());
    assert!(!PathBuf::from("/Volumes/SummonrNoSuchVolume").exists());
}

#[test]
fn todo_accepts_the_todos_key_models_actually_send() {
    let items =
        todos_from_args(r#"{"todos":[{"title":"decode the image","status":"in_progress"}]}"#)
            .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].text, "decode the image");
    assert_eq!(items[0].status, TodoStatus::Active);
}

fn workspace(files: &[(&str, &str)]) -> (tempfile::TempDir, ToolRegistry) {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let tools = ToolRegistry::with_policy_and_search(dir.path(), Policy::builtin(), None).unwrap();
    (dir, tools)
}

fn run(tools: &ToolRegistry, name: &str, arguments: serde_json::Value) -> String {
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
        continuation: None,
    };
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call))
        .unwrap()
        .text
}

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
fn output_under_the_budget_comes_back_whole_with_no_log_note() {
    let (_dir, tools) = workspace(&[]);
    let text = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "seq 1 300"}),
    );
    assert!(text.contains("\n150\n") && text.contains("300"), "{text}");
    assert!(!text.contains("read the full output"), "{text}");
}

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
fn ls_runs_in_the_shell_instead_of_turning_into_a_read() {
    let (_dir, tools) = workspace(&[("a.txt", "x\n")]);
    let text = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "ls -l a.txt"}),
    );
    assert!(text.contains("-rw"), "{text}");
    assert!(!text.contains("answers this natively"), "{text}");
}

#[test]
fn a_command_past_the_wait_limit_keeps_running_as_a_job() {
    let (_dir, mut tools) = workspace(&[]);
    let user = [
        (crate::modules::ModuleId::Search, false),
        (crate::modules::ModuleId::Lsp, false),
        (crate::modules::ModuleId::Jobs, true),
        (crate::modules::ModuleId::Skills, false),
    ]
    .into();
    tools.set_modules(&crate::modules::Switches::from_maps(
        &user,
        &std::collections::BTreeMap::new(),
    ));
    tools.set_session("s1");
    let started = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "sleep 2 && echo finished"}),
    );
    let id = started
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .unwrap_or_else(|| panic!("no job id in {started}"))
        .to_string();
    assert!(
        started.contains(&format!("run_command {{\"job\":\"{id}\"}}")),
        "{started}"
    );
    let polled = run(&tools, "run_command", serde_json::json!({"job": id}));
    assert!(polled.contains("finished"), "{polled}");
}

#[test]
fn todo_is_advertised_only_where_the_ledger_is_read() {
    let (_dir, tools) = workspace(&[]);
    let names = |work| {
        tools
            .definitions(work)
            .into_iter()
            .map(|tool| tool.function.name)
            .collect::<Vec<_>>()
    };
    assert!(
        !names(crate::durable::work::WorkKind::Single)
            .iter()
            .any(|name| name == "todo")
    );
    assert!(
        names(crate::durable::work::WorkKind::Steps)
            .iter()
            .any(|name| name == "todo")
    );
}

#[test]
fn web_search_is_off_unless_turned_on() {
    let empty = std::collections::BTreeMap::new();
    assert!(
        !crate::modules::Switches::from_maps(&empty, &empty)
            .enabled(crate::modules::ModuleId::Search)
    );
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
fn reading_an_image_hands_it_to_a_model_that_sees_images() {
    let (dir, mut tools) = workspace(&[]);
    let mut png = Vec::new();
    ::image::RgbImage::new(2, 2)
        .write_to(
            &mut std::io::Cursor::new(&mut png),
            ::image::ImageFormat::Png,
        )
        .unwrap();
    std::fs::write(dir.path().join("board.png"), &png).unwrap();
    tools.set_images(true, None);
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: serde_json::json!({"path": "board.png"}).to_string(),
        },
        continuation: None,
    };
    let output = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call))
        .unwrap();
    assert_eq!(output.images.len(), 1, "{}", output.text);
    assert!(
        !output.text.contains("shown to the user"),
        "{}",
        output.text
    );
}

#[test]
fn a_long_output_line_reaches_the_model_whole() {
    let (_dir, tools) = workspace(&[]);
    let text = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "printf 'x%.0s' $(seq 1 2000); echo"}),
    );
    assert!(!text.contains("clipped"), "{text}");
    assert!(!text.contains("full output"), "{text}");
    assert!(text.contains(&"x".repeat(2000)), "{} bytes", text.len());
}

#[test]
fn a_cat_with_a_workdir_reads_from_that_directory() {
    let (dir, tools) = workspace(&[("note.txt", "top\n")]);
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/note.txt"), "inner\n").unwrap();
    let text = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "cat note.txt", "workdir": "sub"}),
    );
    assert!(text.contains("inner"), "{text}");
}

#[test]
fn a_file_read_with_context_widens_the_page_around_the_offset() {
    let body: String = (1..2000).map(|n| format!("row{n}\n")).collect();
    let (_dir, tools) = workspace(&[("big.py", &body)]);
    let text = run(
        &tools,
        "read",
        serde_json::json!({"op": "file", "path": "big.py", "offset": 100, "limit": 2, "before": 2, "after": 3}),
    );
    assert!(text.contains("row98") && text.contains("row104"), "{text}");
    assert!(
        !text.contains("row97") && !text.contains("row105"),
        "{text}"
    );
}

#[test]
fn a_command_that_cannot_become_a_job_outlives_the_job_wait() {
    let (_dir, mut tools) = workspace(&[]);
    let user = [(crate::modules::ModuleId::Jobs, false)].into();
    tools.set_modules(&crate::modules::Switches::from_maps(
        &user,
        &std::collections::BTreeMap::new(),
    ));
    tools.set_session("s1");
    let output = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "sleep 2 && echo finished"}),
    );
    assert!(output.starts_with("exit 0"), "{output}");
    assert!(output.contains("finished"), "{output}");
}

#[test]
fn the_full_output_note_counts_every_line_of_a_large_log() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("out.log");
    let line = "x".repeat(1023) + "\n";
    std::fs::write(&log, line.repeat(700)).unwrap();
    let note = spill::full_output_note(&log, true);
    assert!(note.ends_with("— 700 lines]"), "got {note}");
    assert_eq!(spill::full_output_note(&log, false), "");
}

#[cfg(sandboxed)]
#[test]
fn a_contained_read_only_registry_runs_read_commands_and_refuses_writes() {
    let (dir, mut tools) = workspace(&[("notes.txt", "one\ntwo\n")]);
    tools.contain_read_only();
    let counted = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "wc -l notes.txt"}),
    );
    assert!(counted.contains("2 notes.txt"), "{counted}");
    let call = ToolCall {
        id: "t2".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command": "echo x > made.txt"}).to_string(),
        },
        continuation: None,
    };
    drop(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(tools.execute_outcome(&call)),
    );
    assert!(!dir.path().join("made.txt").exists());
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

fn outcome(tools: &ToolRegistry, command: &str) -> String {
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({ "command": command }).to_string(),
        },
        continuation: None,
    };
    let result = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call));
    match result {
        Ok(output) => output.text,
        Err(error) => error.to_string(),
    }
}

#[test]
fn a_contained_session_asks_before_a_hazard_and_strict_refuses_it() {
    let (_dir, mut tools) = workspace(&[]);
    for command in ["sudo true", "bash <<'EOF'\nsudo true\nEOF"] {
        let asked = outcome(&tools, command);
        assert!(asked.contains("needs the user's approval"), "{asked}");
    }
    let written = outcome(&tools, "cat > notes.md <<'EOF'\nsudo true\nEOF");
    assert!(!written.contains("needs the user's approval"), "{written}");
    tools.set_strict();
    let refused = outcome(&tools, "sudo true");
    assert!(refused.contains("strict mode never runs it"), "{refused}");
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

#[test]
fn a_command_in_another_workdir_is_judged_from_that_directory() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("ws");
    std::fs::create_dir_all(workspace.join("keep")).unwrap();
    std::fs::create_dir_all(root.path().join("keep")).unwrap();
    let tools = ToolRegistry::with_policy_and_search(&workspace, Policy::builtin(), None).unwrap();
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "rm -rf keep",
                "workdir": root.path().display().to_string(),
            })
            .to_string(),
        },
        continuation: None,
    };
    let result = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call));
    let text = match result {
        Ok(output) => output.text,
        Err(error) => error.to_string(),
    };
    assert!(root.path().join("keep").exists(), "{text}");
}
