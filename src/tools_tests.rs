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
    tools.set_modules(false, false, true, false);
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
    assert!(!crate::modules::Switches::from_maps(&empty, &empty).enabled("search"));
}
