use super::*;

#[test]
fn task_advances_one_step_at_a_time() -> Result<()> {
    let (state, _) = TaskState::transition(
        None,
        r#"{"action":"start","steps":["Inspect", "Edit", "Verify"]}"#,
    )?;
    assert_eq!(
        state.to_string(),
        "[>] 1. Inspect\n[ ] 2. Edit\n[ ] 3. Verify"
    );

    let (state, _) = TaskState::transition(Some(&state), r#"{"action":"advance"}"#)?;
    assert_eq!(
        state.to_string(),
        "[x] 1. Inspect\n[>] 2. Edit\n[ ] 3. Verify"
    );
    assert!(!state.is_complete());
    let (state, _) = TaskState::transition(Some(&state), r#"{"action":"advance"}"#)?;
    let (state, text) = TaskState::transition(Some(&state), r#"{"action":"advance"}"#)?;
    assert_eq!(text, "completed task");
    assert!(state.is_complete());
    Ok(())
}

#[test]
fn revise_preserves_completed_steps() -> Result<()> {
    let (state, _) = TaskState::transition(
        None,
        r#"{"action":"start","steps":["Inspect", "Edit", "Verify"]}"#,
    )?;
    let (state, _) = TaskState::transition(Some(&state), r#"{"action":"advance"}"#)?;
    let (revised, _) = TaskState::transition(
        Some(&state),
        r#"{"action":"revise","steps":["Inspect", "Change parser", "Test parser", "Verify"]}"#,
    )?;
    assert_eq!(revised.current(), Some("Change parser"));
    TaskState::transition(
        Some(&state),
        r#"{"action":"revise","steps":["Explore", "Edit"]}"#,
    )
    .unwrap_err();
    Ok(())
}

#[test]
fn only_file_mutations_require_task_state() {
    assert!(guards("edit_file"));
    assert!(guards("write_file"));
    assert!(!guards("run_command"));
}
