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

#[test]
fn job_state_survives_restart_and_keeps_accounting() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("mage-step-job-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir)?;
    let plan = dir.join("PLAN.md");
    std::fs::write(&plan, "## Steps\n\n1. first\n2. second\n")?;

    let mut job = Job::open(&dir, &plan, "session-1", Some(500), Some(60), None)?;
    job.validate_steps(2)?;
    job.complete_step(1, 123, 7)?;
    assert_eq!(job.state().next_step, 2);
    assert_eq!(job.state().last_completed_step, Some(1));
    assert_eq!(job.state().spent_tokens, 123);
    assert_eq!(job.state().elapsed_secs, 7);
    assert!(job.state().terminal.is_none());

    let snapshot = job.state().clone();
    let mut resumed = Job::from_state(dir.clone(), snapshot);
    resumed.finish(Terminal::Completed, 200, 11)?;
    assert_eq!(resumed.state().terminal, Some(Terminal::Completed));
    assert_eq!(resumed.state().spent_tokens, 200);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn job_open_ignores_leftover_mage_jobs_files() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("mage-step-jobs-ignore-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir)?;
    let plan = dir.join("PLAN.md");
    std::fs::write(&plan, "## Steps\n\n1. first\n")?;
    let jobs = dir.join(".mage").join("jobs");
    std::fs::create_dir_all(&jobs)?;
    std::fs::write(jobs.join("steps-stale.json"), r#"{"next_step":99}"#)?;
    let job = Job::open(&dir, &plan, "session-1", None, None, None)?;
    assert_eq!(job.state().next_step, 1);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn job_state_rejects_a_changed_plan() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("mage-step-job-plan-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir)?;
    let plan = dir.join("PLAN.md");
    std::fs::write(&plan, "## Steps\n\n1. first\n")?;
    let job = Job::open(&dir, &plan, "session-1", None, None, None)?;
    let original = job.state().plan_hash.clone();
    std::fs::write(&plan, "## Steps\n\n1. changed\n")?;
    let changed = Job::open(&dir, &plan, "session-1", None, None, None)?;
    assert_ne!(
        changed.state().plan_hash,
        original,
        "a changed plan must hash differently"
    );
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn job_state_rejects_a_changed_workspace_identity() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("mage-step-job-workspace-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir)?;
    let plan = dir.join("PLAN.md");
    std::fs::write(&plan, "## Steps\n\n1. first\n")?;
    let job = Job::open(&dir, &plan, "session-1", None, None, None)?;
    assert_ne!(job.state().workspace, "/different/workspace");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn progress_checkpoints_a_new_git_identity() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("mage-step-job-git-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir)?;
    let plan = dir.join("PLAN.md");
    let note = dir.join("note.txt");
    std::fs::write(&plan, "## Steps\n\n1. first\n")?;
    std::fs::write(&note, "first\n")?;
    let git = |args: &[&str]| -> Result<()> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .output()?;
        if output.status.success() {
            return Ok(());
        }
        Err(Error::Tool(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    };
    git(&["init"])?;
    git(&["config", "user.email", "mage-tests@example.invalid"])?;
    git(&["config", "user.name", "Mage Tests"])?;
    git(&["add", "PLAN.md", "note.txt"])?;
    git(&["commit", "-m", "initial"])?;

    let mut job = Job::open(&dir, &plan, "session-1", None, None, None)?;
    std::fs::write(&note, "second\n")?;
    git(&["add", "note.txt"])?;
    git(&["commit", "-m", "second"])?;
    job.progress(10, 1)?;
    assert_eq!(job.state().spent_tokens, 10);
    assert_eq!(job.state().elapsed_secs, 1);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
