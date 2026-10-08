use std::path::PathBuf;

use super::*;
use crate::policy::Policy;

#[path = "tools_tests/command.rs"]
mod command;
#[path = "tools_tests/edit.rs"]
mod edit;
#[path = "tools_tests/read.rs"]
mod read;
#[path = "tools_tests/search.rs"]
mod search;

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

fn names(tools: &ToolRegistry, work: crate::WorkKind) -> Vec<String> {
    tools
        .definitions(work)
        .into_iter()
        .map(|tool| tool.function.name)
        .collect()
}

#[test]
fn plan_mode_keeps_the_tool_list_and_refuses_the_edit_when_it_runs() {
    let (_dir, mut tools) = workspace(&[("a.txt", "one\n")]);
    let open = names(&tools, crate::WorkKind::Single);
    tools.set_read_only(true);
    assert_eq!(names(&tools, crate::WorkKind::Single), open);
    let goal = names(&tools, crate::WorkKind::Goal);
    assert_eq!(goal.last().map(String::as_str), Some("todo"));
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "write".to_string(),
            arguments: serde_json::json!({ "path": "a.txt", "content": "two\n" }).to_string(),
        },
        continuation: None,
    };
    let refused = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call));
    let text = match refused {
        Ok(output) => output.text,
        Err(error) => error.to_string(),
    };
    assert!(text.contains("plan mode does not edit files"), "{text}");
}
