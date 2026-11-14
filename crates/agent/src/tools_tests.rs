use std::path::PathBuf;

use super::*;
use crate::policy::Policy;

#[path = "tools_tests/browser.rs"]
mod browser;
#[path = "tools_tests/command.rs"]
mod command;
#[path = "tools_tests/defaults.rs"]
mod defaults;
#[path = "tools_tests/edit.rs"]
mod edit;
#[path = "tools_tests/read.rs"]
mod read;
#[cfg(unix)]
#[path = "tools_tests/read_links.rs"]
mod read_links;
#[path = "tools_tests/search.rs"]
mod search;
#[path = "tools_tests/search_power.rs"]
mod search_power;
#[path = "tools_tests/shell_search.rs"]
mod shell_search;
#[cfg(windows)]
#[path = "tools_tests/windows.rs"]
mod windows;

#[test]
fn empty_run_command_arguments_are_a_tool_error() {
    let error = parse_args::<Value>("run_command", "").unwrap_err();
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
    let expected = summonr_core::dirs::summonr_home().unwrap().join("spill");
    assert!(path.starts_with(&expected), "{path:?}");
    assert!(path.exists(), "{path:?}");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn a_trimmed_command_keeps_the_complete_log() -> crate::Result<()> {
    let (_dir, tools) = workspace(&[]);
    let text = run(
        &tools,
        "run_command",
        serde_json::json!({"command": per_shell("seq 1 9000", "1..9000")}),
    );
    let path = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("[full output: "))
        .and_then(|rest| rest.split_once(" ("))
        .map(|(path, _)| std::path::PathBuf::from(path))
        .expect("trimmed output names its full log");
    assert!(text.len() < 17_000, "preview was {} bytes", text.len());
    let full = std::fs::read_to_string(path)?;
    assert!(full.lines().any(|line| line == "3000") && full.contains("9000"));
    assert!(!text.lines().any(|line| line == "3000"));
    Ok(())
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
        crate::limits::limits().command_wait().as_secs() + 1
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
    tools.set_scope(crate::policy::Scope::new(
        Some(&["Read".to_string(), "Grep".to_string()]),
        None,
    ));
    let offered: Vec<String> = tools
        .definitions()
        .into_iter()
        .map(|definition| definition.function.name)
        .collect();
    assert!(offered.contains(&"read".to_string()), "{offered:?}");
    assert!(!offered.contains(&"run_command".to_string()), "{offered:?}");
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

#[test]
fn a_read_only_registry_keeps_the_parent_tool_list() {
    let (_dir, mut tools) = workspace(&[]);
    let parent = names(&tools);
    tools.contain_read_only();
    assert_eq!(names(&tools), parent);
}

#[test]
fn read_pages_share_the_output_budget_without_hiding_later_lines() {
    let line = "x".repeat(120);
    let body: String = (0..1000).map(|n| format!("{n:04}{line}\n")).collect();
    let (_dir, tools) = workspace(&[("big.txt", body.as_str())]);
    let text = run(&tools, "read", serde_json::json!({ "path": "big.txt" }));
    assert!(text.len() <= 16 * 1024, "{} bytes", text.len());
    assert!(text.contains("continue with offset"), "{}", &text[..200]);
    let later = run(
        &tools,
        "read",
        serde_json::json!({"path": "big.txt", "offset": 301}),
    );
    assert!(
        later.contains("0300xxxx"),
        "later source lines were hidden: {later}"
    );
}

fn load_skill(dir: &std::path::Path, tools: &ToolRegistry, body: &str) -> String {
    let skill = dir.join(".claude/skills/probe-inject");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), body).unwrap();
    let name = serde_json::json!({ "name": "probe-inject" });
    outcome_of(tools, "skill", name)
}

#[test]
fn a_trusted_skill_gets_each_block_output_in_place_of_the_block() {
    let (dir, mut tools) = workspace(&[]);
    tools.set_trust(true);
    let body = "Branch: !`echo probe-out`\n```!\necho fenced-out\n```\nKEY=!`echo kept`";
    let text = load_skill(dir.path(), &tools, body);
    assert!(text.starts_with("Branch: probe-out\n"), "{text}");
    assert!(text.contains("\nfenced-out\n"), "{text}");
    assert!(!text.contains("```!"), "{text}");
    assert!(text.contains("KEY=!`echo kept`"), "{text}");
}

#[test]
fn a_refused_or_failed_block_stops_the_whole_skill() {
    let (dir, mut tools) = workspace(&[]);
    tools.set_trust(true);
    let refused = load_skill(
        dir.path(),
        &tools,
        "Secret plan\n!`dd if=/dev/zero of=/dev/null count=1`",
    );
    assert!(
        refused.contains("Shell command permission check failed for pattern \"!`dd if=/dev/zero of=/dev/null count=1`\""),
        "{refused}"
    );
    let failed = load_skill(dir.path(), &tools, "Secret plan\n!`exit 3`");
    assert!(failed.contains("command failed for pattern"), "{failed}");
    assert!(!refused.contains("Secret plan") && !failed.contains("Secret plan"));
}

#[test]
fn an_untrusted_workspace_skill_never_runs_its_blocks() {
    let (dir, tools) = workspace(&[]);
    let text = load_skill(dir.path(), &tools, "Mark: !`touch ran.txt`");
    assert_eq!(text, "Mark: [shell command execution disabled by policy]");
    assert!(!dir.path().join("ran.txt").exists());
}

#[test]
fn a_skill_attaches_a_workspace_file_but_not_a_protected_one() {
    let files = [("notes.txt", "probe notes"), (".env", "API_KEY=probe-77")];
    let (dir, tools) = workspace(&files);
    let text = load_skill(dir.path(), &tools, "Read @notes.txt");
    assert!(text.contains("--- notes.txt ---\nprobe notes"), "{text}");
    let refused = load_skill(dir.path(), &tools, "Read @.env");
    assert!(refused.contains("protected path"), "{refused}");
    assert!(!refused.contains("probe-77"), "{refused}");
}
