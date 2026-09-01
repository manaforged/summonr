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
    let receipt = Receipt::new(Vec::new(), Vec::new());
    assert_eq!(
        context("  unchanged\n", &receipt),
        "unchanged\n\nTurn receipt\n- Changed: none\n- Verification: not configured"
    );
    assert_eq!(
        receipt.text,
        "Turn receipt\n- Changed: none\n- Verification: not configured"
    );
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
    );
    let json = serde_json::to_value(&receipt).expect("receipt JSON");
    assert_eq!(json["checks"].as_array().map(Vec::len), Some(2));
    assert_eq!(json["checks"][1]["passed"], false);

    let event = crate::Event::Receipt { receipt };
    let json = serde_json::to_value(event).expect("receipt event JSON");
    assert_eq!(json["type"], "receipt");
    assert_eq!(json["receipt"]["checks"].as_array().map(Vec::len), Some(2));
}
