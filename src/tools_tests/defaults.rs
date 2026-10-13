use super::*;

#[test]
fn a_command_runs_when_the_model_fills_op_with_help() {
    let (_dir, tools) = workspace(&[]);
    let filled = run(
        &tools,
        "run_command",
        serde_json::json!({
            "op": "help",
            "command": "echo filled",
            "background": false,
            "job": "",
            "kill": false,
            "timeout_secs": 120,
        }),
    );
    assert!(filled.contains("filled"), "{filled}");
    assert!(!filled.starts_with("run_command reference."), "{filled}");
    let help = run(&tools, "run_command", serde_json::json!({"op": "help"}));
    assert!(help.starts_with("run_command reference."), "{help}");
}

#[test]
fn filled_defaults_do_not_change_what_run_command_does() {
    let (_dir, tools) = workspace(&[]);
    let zero = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "echo zero", "timeout_secs": 0}),
    );
    assert!(zero.contains("zero"), "{zero}");
    let both = outcome_with(
        &tools,
        serde_json::json!({"command": "echo both", "job": "job-1"}),
    );
    assert!(both.contains("both job `job-1` and a command"), "{both}");
}

#[test]
fn filled_defaults_do_not_change_what_read_and_edit_do() {
    let (dir, tools) = workspace(&[("a.txt", "alpha\n")]);
    let read = run(
        &tools,
        "read",
        serde_json::json!({"op": "help", "path": "a.txt"}),
    );
    assert!(read.contains("alpha"), "{read}");
    let tagged = run(
        &tools,
        "edit",
        serde_json::json!({"op": "help", "path": "a.txt", "old": "alpha", "new": "beta", "tag": "",
            "edits": [{"old": "", "new": ""}]}),
    );
    assert!(!tagged.contains("tag must be"), "{tagged}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "beta\n"
    );
}
