use std::path::PathBuf;

use super::*;
use crate::policy::Policy;

#[path = "tools_tests/browser.rs"]
mod browser;
#[path = "tools_tests/command.rs"]
mod command;
#[path = "tools_tests/edit.rs"]
mod edit;
#[path = "tools_tests/read.rs"]
mod read;
#[path = "tools_tests/search.rs"]
mod search;
#[path = "tools_tests/search_power.rs"]
mod search_power;
#[path = "tools_tests/shell_search.rs"]
mod shell_search;

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

#[test]
fn a_standalone_registry_keeps_its_spill_directory() {
    let (dir, mut tools) = workspace(&[]);
    tools.standalone = true;
    let output = tools
        .fit_output(Ok(ToolOutput::ok("x".repeat(80_000))))
        .unwrap();
    assert!(output.text.contains("[full output:"), "{}", output.text);
    let path = output
        .text
        .lines()
        .find_map(|line| line.trim().strip_prefix("[full output: "))
        .and_then(|rest| rest.split(' ').next())
        .map(std::path::PathBuf::from)
        .unwrap();
    drop(tools);
    drop(dir);
    let expected = crate::prompt::summonr_home().unwrap().join("spill");
    assert!(path.starts_with(&expected), "{path:?}");
    assert!(path.exists(), "{path:?}");
    std::fs::remove_file(path).unwrap();
}

fn workspace(files: &[(&str, &str)]) -> (tempfile::TempDir, ToolRegistry) {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let tools = ToolRegistry::with_policy_and_search(dir.path(), Policy::builtin(), None).unwrap();
    (dir, tools)
}

fn past_the_wait() -> String {
    format!(
        "sleep {}; echo finished",
        crate::limits::limits().command_wait_secs + 1
    )
}

fn per_shell(bash: &'static str, powershell: &'static str) -> &'static str {
    if cfg!(windows) { powershell } else { bash }
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
    outcome_with(tools, serde_json::json!({ "command": command }))
}

fn outcome_with(tools: &ToolRegistry, arguments: serde_json::Value) -> String {
    outcome_of(tools, "run_command", arguments)
}

fn outcome_of(tools: &ToolRegistry, name: &str, arguments: serde_json::Value) -> String {
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

fn names(tools: &ToolRegistry) -> Vec<String> {
    tools
        .definitions()
        .into_iter()
        .map(|tool| tool.function.name)
        .collect()
}

#[test]
fn a_read_only_session_keeps_the_tool_list_and_refuses_the_edit_when_it_runs() {
    let (_dir, mut tools) = workspace(&[("a.txt", "one\n")]);
    let open = names(&tools);
    tools.set_read_only(true);
    assert_eq!(names(&tools), open);
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
    assert!(text.contains("this session is read-only"), "{text}");
}

#[tokio::test]
async fn an_agent_tool_list_denies_tools_outside_it_by_claude_or_summonr_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "hello\n").unwrap();
    let mut tools =
        ToolRegistry::with_policy_and_search(dir.path(), Policy::builtin(), None).unwrap();
    tools.restrict_to(&["Read".to_string(), "Grep".to_string()]);
    let call = |name: &str, arguments: serde_json::Value| crate::provider::ToolCall {
        id: "t".to_string(),
        kind: crate::provider::ToolKind::Function,
        function: crate::provider::ToolFunction {
            checked: Default::default(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
        continuation: None,
    };
    let read = tools
        .execute_outcome(&call("read", serde_json::json!({"path": "note.txt"})))
        .await
        .unwrap();
    assert!(read.text.contains("hello"), "{}", read.text);
    let denied = tools
        .execute_outcome(&call("run_command", serde_json::json!({"command": "true"})))
        .await;
    assert!(
        matches!(denied, Err(Error::Denied(_))),
        "{:?}",
        denied.err()
    );
}

#[tokio::test]
async fn the_model_cannot_write_the_check_file_in_any_mode() {
    let (dir, mut tools) = workspace(&[]);
    tools.set_unrestricted();
    std::fs::create_dir_all(dir.path().join(".summonr")).unwrap();
    std::fs::write(
        dir.path().join(".summonr/check.toml"),
        "check = \"false\"\n",
    )
    .unwrap();
    assert!(tools.policy().is_unrestricted());
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "write".to_string(),
            arguments: serde_json::json!({ "path": ".summonr/check.toml", "content": "check = \"true\"\n" })
                .to_string(),
        },
        continuation: None,
    };
    let text = match tools.execute_outcome(&call).await {
        Ok(output) => output.text,
        Err(error) => error.to_string(),
    };
    assert!(text.contains("holds the run's check"), "{text}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".summonr/check.toml")).unwrap(),
        "check = \"false\"\n"
    );
}
