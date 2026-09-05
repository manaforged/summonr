use super::*;

#[test]
fn check_commands_read_workspace_toml() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("mage-check-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".mage"))?;
    assert_eq!(check_commands(&dir)?, None);
    std::fs::write(dir.join(".mage/check.toml"), "check = \"cargo test\"")?;
    assert_eq!(check_commands(&dir)?, Some(vec!["cargo test".to_string()]));
    std::fs::write(
        dir.join(".mage/check.toml"),
        "checks = [\"cargo clippy\", \"cargo test\"]",
    )?;
    assert_eq!(
        check_commands(&dir)?,
        Some(vec!["cargo clippy".to_string(), "cargo test".to_string()])
    );
    std::fs::write(
        dir.join(".mage/check.toml"),
        "check = \"a\"\nchecks = [\"b\"]",
    )?;
    check_commands(&dir).unwrap_err();
    std::fs::write(dir.join(".mage/check.toml"), "check = \"  \"")?;
    check_commands(&dir).unwrap_err();
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn receipt_leads_with_a_failed_check_and_names_the_changed_file() {
    let receipt = Receipt::new(
        vec!["src/main.rs".to_string()],
        vec![Outcome {
            command: "cargo test".to_string(),
            passed: false,
            output: "exit 101\nfailed".to_string(),
        }],
        "abc123 (HEAD), worktree clean".to_string(),
        &[],
    );
    let text = context("done", &receipt);

    assert!(text.starts_with("Verification failed."));
    assert_eq!(receipt.checks.len(), 1);
    assert!(text.contains("`src/main.rs`"));
    assert!(text.contains("`cargo test` failed"));
    assert!(text.contains("    exit 101"));
}

#[test]
fn receipt_records_an_unchanged_unverified_turn() {
    let receipt = Receipt::new(Vec::new(), Vec::new(), "non-Git workspace".to_string(), &[]);
    assert_eq!(
        context("  unchanged\n", &receipt),
        "unchanged\n\nTurn receipt\n- Started at: non-Git workspace\n- Inspected: 0 files read, 0 searches, 0 commands\n- Changed: none\n- Verification: not configured"
    );
    assert_eq!(
        receipt.text,
        "Turn receipt\n- Started at: non-Git workspace\n- Inspected: 0 files read, 0 searches, 0 commands\n- Changed: none\n- Verification: not configured"
    );
}

#[test]
fn aborted_receipt_does_not_claim_changes_were_captured() {
    let receipt = Receipt::aborted("abc123 (HEAD)".to_string(), &[]);

    assert!(receipt.aborted);
    assert!(receipt.text.contains("Changed: not captured before abort"));
    assert!(receipt.text.contains("Verification: not run"));
}

#[test]
fn receipt_serializes_every_check() {
    let receipt = Receipt::new(
        vec!["src/lib.rs".to_string()],
        vec![
            Outcome {
                command: "cargo check".to_string(),
                passed: true,
                output: String::new(),
            },
            Outcome {
                command: "cargo test".to_string(),
                passed: false,
                output: "failed".to_string(),
            },
        ],
        "abc123 (HEAD), worktree clean".to_string(),
        &[],
    );
    let json = serde_json::to_value(&receipt).expect("receipt JSON");
    assert_eq!(json["checks"].as_array().map(Vec::len), Some(2));
    assert_eq!(json["checks"][1]["passed"], false);

    let event = crate::Event::Receipt { receipt };
    let json = serde_json::to_value(event).expect("receipt event JSON");
    assert_eq!(json["type"], "receipt");
    assert_eq!(json["receipt"]["checks"].as_array().map(Vec::len), Some(2));
}

#[test]
fn receipt_counts_unique_reads_and_names_failed_tools() {
    let events = vec![
        Event::ToolCall {
            id: "one".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"src/lib.rs"}"#.to_string(),
        },
        Event::ToolCall {
            id: "two".to_string(),
            name: "read_file".to_string(),
            arguments: r#"{"path":"missing.rs"}"#.to_string(),
        },
        Event::ToolCall {
            id: "three".to_string(),
            name: "grep".to_string(),
            arguments: "{}".to_string(),
        },
        Event::ToolCall {
            id: "four".to_string(),
            name: "web_fetch".to_string(),
            arguments: "{}".to_string(),
        },
        Event::ToolCall {
            id: "five".to_string(),
            name: "run_command".to_string(),
            arguments: "{}".to_string(),
        },
        Event::ToolResult {
            id: "one".to_string(),
            success: true,
            mutated: false,
            output: String::new(),
        },
        Event::ToolResult {
            id: "two".to_string(),
            success: false,
            mutated: false,
            output: "not found".to_string(),
        },
        Event::ToolResult {
            id: "five".to_string(),
            success: false,
            mutated: false,
            output: "timed out\nmore".to_string(),
        },
    ];
    let receipt = Receipt::new(Vec::new(), Vec::new(), "abc123 (HEAD)".to_string(), &events);

    assert_eq!(receipt.reads, 1);
    assert_eq!(receipt.searches, 2);
    assert_eq!(receipt.commands, 1);
    assert_eq!(
        receipt.failures,
        ["read_file: not found", "run_command: timed out"]
    );
    assert!(
        receipt
            .text
            .contains("Inspected: 1 file read, 2 searches, 1 command")
    );
    assert!(
        receipt
            .text
            .contains("Failed: 2 — read_file: not found; run_command: timed out")
    );
}
