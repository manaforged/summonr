use super::*;
use crate::steps::Terminal;

#[test]
fn verdict_parses_clean_json() {
    let verdict = parse_verdict("{\"done\": true, \"reason\": \"tests pass\"}");
    assert!(verdict.done);
    assert_eq!(verdict.reason, "tests pass");

    let verdict = parse_verdict("{\"done\": false, \"reason\": \"two files remain\"}");
    assert!(!verdict.done);
    assert_eq!(verdict.reason, "two files remain");
}

#[test]
fn verdict_tolerates_prose_and_garbage() {
    let verdict = parse_verdict("Sure! {\"done\": true, \"reason\": \"built\"} hope that helps");
    assert!(verdict.done);
    assert_eq!(verdict.reason, "built");

    // Unparsable replies fail open as "continue".
    let verdict = parse_verdict("the objective looks incomplete");
    assert!(!verdict.done);
    assert_eq!(verdict.reason, "the objective looks incomplete");
    let verdict = parse_verdict("{\"done\": \"yes\"}");
    assert!(!verdict.done);
}

#[test]
fn opening_prompt_names_the_gates() {
    let text = opening_prompt("fix the flaky tests", &["cargo test".to_string()]);
    assert!(text.contains("fix the flaky tests"));
    assert!(text.contains("`cargo test`"));
    let text = opening_prompt("fix the flaky tests", &[]);
    assert!(!text.contains("check"));
}

#[test]
fn continuation_prompt_carries_the_judge_reason() {
    let text = continuation_prompt("CI is still red");
    assert!(text.contains("CI is still red"));
    assert!(text.starts_with("[summon goal:"));
}

/// The turn counter and terminal states without a live loop: drive the
/// job directly the way `after_turn` does.
#[test]
fn engine_counts_turns_and_pauses_at_the_budget() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("mage-goal-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let objective = "add a CHANGELOG entry";
    let mut engine = Engine::open(
        &dir,
        objective,
        "session-1",
        Vec::new(),
        2,
        &crate::Budget::none(),
    )?;
    assert_eq!(
        engine.prompt()?.as_deref(),
        Some(opening_prompt(objective, &[]).as_str())
    );

    // Simulate two completed turns.
    for _ in 0..2 {
        let index = engine.job.state().next_step;
        engine.job.complete_step(index, 10, 1)?;
        engine.pending = Some(continuation_prompt("not yet"));
    }
    // Third prompt exceeds max_turns 2 → aborted pause.
    assert!(engine.prompt()?.is_none());
    assert_eq!(
        engine.exit_code(),
        Some(
            Terminal::Aborted {
                reason: String::new()
            }
            .exit_code()
        )
    );
    let mut resumed =
        Engine::from_state(&dir, objective, Vec::new(), 2, engine.snapshot().clone())?;
    assert!(resumed.prompt()?.is_none());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn engine_restores_feedback_strikes_and_turn_limit() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("mage-goal-resume-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let objective = "finish the refactor";
    let mut engine = Engine::open(
        &dir,
        objective,
        "session-1",
        vec!["cargo test".to_string()],
        7,
        &crate::Budget::none(),
    )?;
    engine.pending = Some(continuation_prompt("one test remains"));
    engine.red_streak = 2;
    engine
        .job
        .complete_goal(1, engine.pending.clone(), engine.red_streak, None, 0, 0)?;

    let mut resumed = Engine::from_state(
        &dir,
        objective,
        vec!["cargo test".to_string()],
        7,
        engine.snapshot().clone(),
    )?;
    assert_eq!(resumed.red_streak, 2);
    assert!(
        resumed
            .prompt()?
            .as_deref()
            .is_some_and(|prompt| { prompt.contains("one test remains") })
    );
    let changed = Engine::from_state(
        &dir,
        objective,
        vec!["cargo test".to_string()],
        8,
        engine.snapshot().clone(),
    );
    assert!(changed.is_err(), "a resumed goal must keep its turn limit");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn terminal_save_failure_is_reported() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("mage-goal-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let mut engine = Engine::open(
        &dir,
        "finish the refactor",
        "session-1",
        Vec::new(),
        0,
        &crate::Budget::none(),
    )?;
    assert!(engine.prompt()?.is_none());
    assert_eq!(engine.exit_code(), Some(3));
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn failed_goal_checkpoint_does_not_advance_memory() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("mage-goal-checkpoint-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let mut engine = Engine::open(
        &dir,
        "finish the refactor",
        "session-1",
        vec!["cargo test".to_string()],
        7,
        &crate::Budget::none(),
    )?;
    engine.job.complete_goal(
        1,
        Some(continuation_prompt("one test remains")),
        2,
        None,
        10,
        1,
    )?;
    assert_eq!(engine.job.state().next_step, 2);
    assert!(engine.job.state().goal_pending.is_some());
    assert_eq!(engine.job.state().goal_red_streak, 2);
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn engine_rejects_an_empty_goal() {
    let dir = std::env::temp_dir().join("mage-goal-empty");
    let error = Engine::open(
        &dir,
        "   ",
        "s",
        Vec::new(),
        DEFAULT_MAX_TURNS,
        &crate::Budget::none(),
    )
    .err()
    .unwrap_or_else(|| panic!("empty goal must be rejected"));
    assert!(error.to_string().contains("empty"));
}

#[test]
fn load_returns_none_without_a_job() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("mage-goal-load-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    assert!(Engine::load(&dir, "no such goal")?.is_none());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
