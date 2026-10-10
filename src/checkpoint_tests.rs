use super::*;

#[test]
fn a_new_session_index_starts_from_the_repository_index() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(workspace)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(workspace.join("a.txt"), "a\n").unwrap();
    git(&["add", "a.txt"]);
    let checkpoints = Checkpoints::open(workspace, "s1").unwrap();
    assert_eq!(
        std::fs::read(&checkpoints.index).unwrap(),
        std::fs::read(workspace.join(".git/index")).unwrap()
    );
}

#[test]
fn restoring_a_turn_removes_unicode_additions_and_preserves_the_user_index() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let workspace = dir.path();
    git_env(workspace, &[], &["init", "-q"])?;
    git_env(workspace, &[], &["config", "core.quotepath", "true"])?;
    git_env(workspace, &[], &["config", "core.autocrlf", "false"])?;
    let original = workspace.join("original.txt");
    std::fs::write(&original, "staged\n")?;
    git_env(workspace, &[], &["add", "original.txt"])?;
    let index = std::fs::read(workspace.join(".git/index"))?;
    std::fs::write(&original, "before\n")?;
    let checkpoints =
        Checkpoints::open(workspace, "restore").expect("a Git workspace supports checkpoints");
    checkpoints.capture()?;
    let added = workspace.join("café.txt");
    std::fs::write(&added, "added\n")?;
    std::fs::write(&original, "after\n")?;

    checkpoints.restore(1)?;

    assert!(!added.exists(), "undo left the added Unicode file behind");
    assert_eq!(std::fs::read_to_string(original)?, "before\n");
    assert_eq!(std::fs::read(workspace.join(".git/index"))?, index);
    Ok(())
}

#[test]
fn a_recorded_output_file_stays_out_of_every_checkpoint() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let workspace = dir.path();
    git_env(workspace, &[], &["init", "-q"])?;
    std::fs::write(workspace.join("a.txt"), "a\n")?;
    std::fs::write(workspace.join("events.jsonl"), "{}\n")?;
    let checkpoints = Checkpoints::open(workspace, "s1").unwrap();
    std::fs::write(&checkpoints.skip, "events.jsonl\n")?;
    checkpoints.capture()?;
    let tree = checkpoints.trees().pop().unwrap();
    let listed = git_env(workspace, &[], &["ls-tree", "--name-only", &tree])?;
    assert!(listed.contains("a.txt"), "{listed}");
    assert!(!listed.contains("events.jsonl"), "{listed}");
    Ok(())
}

#[test]
fn a_file_staged_before_it_was_skipped_leaves_the_next_checkpoint() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let workspace = dir.path();
    git_env(workspace, &[], &["init", "-q"])?;
    std::fs::write(workspace.join("diff.json"), "{}\n")?;
    let checkpoints = Checkpoints::open(workspace, "s1").unwrap();
    checkpoints.capture()?;
    std::fs::write(workspace.join("diff.json"), "")?;
    std::fs::write(&checkpoints.skip, "diff.json\n")?;
    let later = Checkpoints::open(workspace, "s2").unwrap();
    later.capture()?;
    checkpoints.capture()?;
    let tree = checkpoints.trees().pop().unwrap();
    let listed = git_env(workspace, &[], &["ls-tree", "--name-only", &tree])?;
    assert!(!listed.contains("diff.json"), "{listed}");
    Ok(())
}

#[test]
fn an_undo_reports_only_the_files_a_user_would_see() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let workspace = dir.path();
    git_env(workspace, &[], &["init", "-q"])?;
    std::fs::write(workspace.join("calc.py"), "before\n")?;
    let checkpoints =
        Checkpoints::open(workspace, "undo").expect("a Git workspace supports checkpoints");
    checkpoints.capture()?;
    std::fs::write(workspace.join("calc.py"), "after\n")?;
    std::fs::create_dir(workspace.join("__pycache__"))?;
    std::fs::write(workspace.join("__pycache__/calc.cpython-312.pyc"), "x")?;
    assert_eq!(checkpoints.restore(1)?, vec!["calc.py".to_string()]);
    assert_eq!(
        std::fs::read_to_string(workspace.join("calc.py"))?,
        "before\n"
    );
    Ok(())
}
