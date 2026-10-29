use super::{Candidate, Decision, StepState, Work, WorkKind};
use crate::durable::log;
use crate::durable::tree::snapshot;
use crate::tools::{Todo, TodoStatus};

fn todo(text: &str, status: TodoStatus) -> Todo {
    Todo {
        text: text.to_string(),
        status,
    }
}

#[test]
fn todo_list_never_switches_off_the_completion_check() {
    let mut work = Work::new("ship", WorkKind::Single);
    work.select("ship", &["cargo test".to_string()]).unwrap();
    work.apply_todos(vec![
        todo("decode the image", TodoStatus::Done),
        todo("write the overlay", TodoStatus::Active),
    ]);
    assert_eq!(work.policy, WorkKind::Single);
    assert_eq!(work.completion_checks().len(), 1);
    assert_eq!(work.progress().todos.len(), 2);
}

#[test]
fn todo_marked_done_cannot_finish_a_steps_plan() {
    let dir = tempfile::tempdir().unwrap();
    let shot = snapshot(dir.path(), &[], None).unwrap();
    let mut work = Work::new("plan", WorkKind::Steps);
    work.select(
        "## Steps\n1. first\n2. second\n",
        &["cargo test".to_string()],
    )
    .unwrap();
    work.execute(Some(shot.clone())).unwrap();
    work.apply_todos(vec![
        todo("first", TodoStatus::Done),
        todo("second", TodoStatus::Done),
    ]);
    work.verify(shot.clone()).unwrap();
    work.verdict(true, Decision::Automatic).unwrap();
    assert!(!work.terminal());
    assert_eq!(work.steps[1].state, StepState::Active);
    assert_eq!(work.completion_checks().len(), 1);
}

#[test]
fn todo_marked_done_never_overrides_the_goal_judge() {
    let dir = tempfile::tempdir().unwrap();
    let shot = snapshot(dir.path(), &[], None).unwrap();
    let mut work = Work::new("make the check pass", WorkKind::Goal);
    work.plan = Some("plan".to_string());
    work.execute(Some(shot.clone())).unwrap();
    work.apply_todos(vec![todo("everything", TodoStatus::Done)]);
    work.verify(shot.clone()).unwrap();
    work.verdict(
        true,
        Decision::GoalContinue("tests still missing".to_string()),
    )
    .unwrap();
    assert!(!work.terminal());
}

#[test]
fn work_trees_live_in_the_sidecar_and_an_old_inline_file_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    let tree = snapshot(dir.path(), &[], None).unwrap();
    let mut work = Work::new("ship", WorkKind::Single);
    work.base = Some(tree.clone());
    work.candidate = Some(Candidate {
        paths: vec!["a.txt".to_string()],
        snapshot: tree.clone(),
    });
    let session = dir.path().join("session.jsonl");
    save(&work, &session);

    let text = std::fs::read_to_string(&session).unwrap();
    let main: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert!(
        main["work"].get("base").is_none(),
        "the work record carries no file map: {main}"
    );
    assert!(
        main["work"]["candidate"].get("snapshot").is_none(),
        "{main}"
    );
    assert!(
        log::work_path(&session)
            .with_extension("tree.json")
            .is_file()
    );
    let loaded = Work::at(&session).unwrap();
    assert_eq!(loaded.base, Some(tree.clone()));
    assert_eq!(loaded.candidate.unwrap().snapshot, tree);

    let old = tempfile::tempdir().unwrap();
    let old_session = old.path().join("session.jsonl");
    let mut inline = serde_json::to_value(&work).unwrap();
    inline["base"] = serde_json::to_value(&tree).unwrap();
    inline["candidate"]["snapshot"] = serde_json::to_value(&tree).unwrap();
    std::fs::write(
        log::work_path(&old_session),
        serde_json::to_vec(&inline).unwrap(),
    )
    .unwrap();
    let legacy = Work::at(&old_session).unwrap();
    assert_eq!(legacy.base, Some(tree.clone()));
    assert_eq!(legacy.candidate.unwrap().snapshot, tree);
}

#[test]
fn an_empty_tree_pair_writes_no_sidecar_and_clears_a_stale_one() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.jsonl");
    let sidecar = log::work_path(&session).with_extension("tree.json");
    let mut work = Work::new("ship", WorkKind::Single);
    work.save_trees(&session).unwrap();
    assert!(!sidecar.exists(), "no file map means no sidecar write");
    work.base = Some(snapshot(dir.path(), &[], None).unwrap());
    work.save_trees(&session).unwrap();
    assert!(sidecar.is_file());
    work.base = None;
    work.save_trees(&session).unwrap();
    assert!(
        !sidecar.exists(),
        "an emptied map removes the stale sidecar"
    );
}

#[test]
fn numbered_steps_stop_after_three_failed_checks() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    let shot = snapshot(dir.path(), &[], None).unwrap();
    let mut work = Work::new("make the check pass", WorkKind::Steps);
    work.select("## Steps\n1. Repair the check\n", &[])
        .expect("select numbered steps");
    work.execute(Some(shot.clone())).unwrap();
    for _ in 0..3 {
        work.verify(shot.clone()).unwrap();
        work.verdict(false, Decision::Automatic).unwrap();
    }
    assert_eq!(work.progress().exit_code(), Some(2));
}

#[test]
fn check_file_and_cache_files_are_never_touched_paths() {
    let dir = tempfile::tempdir().unwrap();
    let write = |path: &str, text: &str| std::fs::write(dir.path().join(path), text).unwrap();
    crate::diff::git_env(dir.path(), &[], &["init", "-q"]).unwrap();
    std::fs::create_dir_all(dir.path().join(".summonr")).unwrap();
    write(".summonr/check.toml", "check = \"a\"\n");
    write("calc.py", "a");
    let mut work = Work::new("ship", WorkKind::Single);
    work.plan = Some("plan".to_string());
    work.execute(Some(snapshot(dir.path(), &[], None).unwrap()))
        .unwrap();
    write(".summonr/check.toml", "check = \"bb\"\n");
    write("calc.py", "bb");
    std::fs::create_dir(dir.path().join("__pycache__")).unwrap();
    write("__pycache__/calc.cpython-312.pyc", "x");
    let paths = work
        .verify(snapshot(dir.path(), &[], None).unwrap())
        .unwrap();
    assert_eq!(paths, vec!["calc.py".to_string()]);
    assert_eq!(work.touched.iter().collect::<Vec<_>>(), vec!["calc.py"]);
}

#[test]
fn an_old_checkpoint_with_a_model_made_step_still_runs_the_saved_checks() {
    let dir = tempfile::tempdir().unwrap();
    let mut work = Work::new("plan", WorkKind::Steps);
    work.select("## Steps\n1. first\n", &["false".to_string()])
        .unwrap();
    let session = dir.path().join("session.jsonl");
    let mut saved = serde_json::to_value(&work).unwrap();
    saved["steps"][0]["title"] = "Investigate".into();
    saved["steps"][0]["checks"] = serde_json::json!([]);
    saved["active"] = 0.into();
    std::fs::write(
        log::work_path(&session),
        serde_json::to_vec(&saved).unwrap(),
    )
    .unwrap();
    let loaded = Work::at(&session).unwrap();
    let commands: Vec<String> = loaded
        .completion_checks()
        .into_iter()
        .map(|check| check.command)
        .collect();
    assert_eq!(commands, ["false"]);
}

#[test]
fn a_torn_work_file_starts_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.jsonl");
    std::fs::write(log::work_path(&session), "").unwrap();
    assert!(Work::at(&session).is_none());
    std::fs::remove_file(log::work_path(&session)).unwrap();
    save(&Work::new("ship it", WorkKind::Single), &session);
    std::fs::write(
        log::work_path(&session).with_extension(super::TREES),
        "{\"base\":",
    )
    .unwrap();
    let loaded = Work::at(&session).expect("the work survives a torn sidecar");
    assert!(loaded.base.is_none());
}

#[test]
fn an_unreadable_work_path_is_an_error_not_a_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.jsonl");
    assert!(matches!(Work::restore(&session, None), Ok(Err(None))));
    std::fs::create_dir(log::work_path(&session)).unwrap();
    let _ = Work::restore(&session, None).unwrap_err();
}

#[test]
fn an_unreadable_tree_sidecar_is_an_error_not_a_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.jsonl");
    std::fs::create_dir(log::work_path(&session).with_extension(super::TREES)).unwrap();
    let _ = Work::restore(&session, Some(Work::new("ship", WorkKind::Single))).unwrap_err();
}

#[test]
fn a_legacy_tree_sidecar_keeps_its_file_map() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session.jsonl");
    save(&Work::new("ship", WorkKind::Single), &session);
    std::fs::write(
        log::work_path(&session).with_extension(super::TREES),
        "{\"base\":{\"a.txt\":[1,2,3]},\"candidate\":null}",
    )
    .unwrap();
    let loaded = Work::at(&session).unwrap();
    let base = loaded.base.expect("the legacy base loads");
    assert_ne!(base, crate::durable::tree::Snapshot::default());
}

fn save(work: &Work, session: &std::path::Path) {
    work.save_trees(session).unwrap();
    if !session.exists() {
        std::fs::write(session, "").unwrap();
    }
    log::Log::open(session)
        .unwrap()
        .append(
            &log::Line::Work {
                work: Box::new(work.clone()),
            },
            crate::durable::store::Durability::Renamed,
        )
        .unwrap();
}
