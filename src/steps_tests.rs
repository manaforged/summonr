use super::*;

const PLAN: &str = "# Thing\n\nprose\n\n## Steps\n\n1. Create `a.rs` with its tests (1–3). Run `cargo test a`.\n2. Create `b.rs`\n   and its tests. Run `cargo test b`.\n3. Run `cargo build` first, then run `cargo test` and print the output.\n4. Tidy up. No check here.\n\n## Done criteria\n\n- all green\n";

#[test]
fn parses_numbered_items_with_continuations_and_checks() {
    let steps = parse(PLAN).unwrap_or_default();
    assert_eq!(steps.len(), 4);
    assert_eq!(steps[0].index, 1);
    assert_eq!(steps[0].checks, vec!["cargo test a"]);
    assert_eq!(
        steps[1].text,
        "Create `b.rs` and its tests. Run `cargo test b`."
    );
    assert_eq!(steps[1].checks, vec!["cargo test b"]);
    assert_eq!(
        steps[2].checks,
        vec!["cargo build"],
        "only Run phrases count"
    );
    assert!(steps[3].checks.is_empty());
    let multi = parse("## Steps\n\n1. Do it. Run `cargo test x`. Then Run `./check.sh`.\n")
        .unwrap_or_default();
    assert_eq!(multi[0].checks, vec!["cargo test x", "./check.sh"]);
}

#[test]
fn phase_headings_group_the_steps() {
    let plan = "## Steps\n\n### Setup\n\n1. Make `a`.\n2. Make `b`.\n\n### Tests\n\n3. Test both. Run `cargo test`.\n\n## Notes\n\n### Not a phase\n";
    let steps = parse(plan).unwrap_or_default();
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0].phase.as_deref(), Some("Setup"));
    assert_eq!(steps[1].phase.as_deref(), Some("Setup"));
    assert_eq!(steps[2].phase.as_deref(), Some("Tests"));
    assert_eq!(steps[2].checks, vec!["cargo test"]);
    let flat = parse(PLAN).unwrap_or_default();
    assert!(flat.iter().all(|step| step.phase.is_none()));
}

#[test]
fn missing_section_is_an_error() {
    parse("# Plan\n\n1. no heading\n").unwrap_err();
    parse("## Steps\n\nnothing numbered\n").unwrap_err();
}

#[test]
fn prompt_and_exit_code() {
    let steps = parse(PLAN).unwrap_or_default();
    let text = prompt(&steps[0], 4, "PLAN.md");
    assert!(text.starts_with("Step 1 of 4 from PLAN.md."));
    assert!(text.contains("run `cargo test a`"));
    assert!(!prompt(&steps[3], 4, "PLAN.md").contains("When you are done"));
    assert!(text.contains("no extra flags, defaults, or features"));
    assert_eq!(exit_code("exit 101\njob x\nlog y\nboom"), Some(101));
    assert_eq!(exit_code("job x"), None);
    assert_eq!(
        exit_code("[3 identical calls in a row]\nexit 0\njob x"),
        Some(0)
    );
    assert_eq!(
        exit_code(
            "(same command 3s ago, no edits since — result repeated, not re-run)\nexit 101\njob x"
        ),
        Some(101)
    );
    assert!(retry_prompt("cargo test", "a\nb").contains("`cargo test` failed"));
}

#[test]
fn contract_parses_and_reports_missing_items() {
    let plan = "## Contract\n\n- file `src/a.rs`\n- file `src/missing.rs`\n- stdout `taskrun --list` → `fetch  fetch inputs`\n- note this bullet is ignored\n\n## Steps\n\n1. x\n";
    let c = contract(plan).unwrap_or_default();
    assert_eq!(c.items.len(), 3);
    assert_eq!(c.items[0], Item::File("src/a.rs".to_string()));
    assert_eq!(
        c.items[2],
        Item::Stdout {
            command: "taskrun --list".to_string(),
            expected: "fetch  fetch inputs".to_string()
        }
    );
    let dir = std::env::temp_dir().join(format!("mage-contract-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(dir.join("src")).unwrap_or_default();
    std::fs::write(dir.join("src/a.rs"), "fn parses_full() {}\n").unwrap_or_default();
    let gone = missing(&c, &dir);
    assert_eq!(gone, vec!["file `src/missing.rs` does not exist"]);
    assert_eq!(stdout_item("x", "abc", "zzabczz"), None);
    assert!(stdout_item("x", "abc", "nope").is_some());
    assert!(contract_prompt(&gone).contains("rename, do not add alternatives"));
    assert!(
        contract("# no section")
            .unwrap_or_default()
            .items
            .is_empty()
    );
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn contract_rejects_source_text_as_proof() {
    let plan = "## Contract\n\n- test `claimed_test`\n- text `required marker`\n";
    let error = match contract(plan) {
        Ok(_) => panic!("source text is not executable proof"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("only `file` and `stdout`"));
}
