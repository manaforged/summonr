use super::{Candidate, Decision, Policy, StepState, Work};
use crate::durable::tree::snapshot;

#[test]
fn a_single_turn_todo_list_survives_work_progress() {
    let mut work = Work::new("ship", Policy::Single);
    work.apply_todos(&[
        ("decode the image".to_string(), StepState::Active),
        ("write the overlay".to_string(), StepState::Pending),
    ]);
    let progress = work.progress();
    assert!(
        progress
            .ledger
            .iter()
            .any(|line| line.contains("[/]") && line.contains("decode the image")),
        "{:?}",
        progress.ledger
    );
    assert!(
        progress
            .ledger
            .iter()
            .any(|line| line.contains("[ ]") && line.contains("write the overlay")),
        "{:?}",
        progress.ledger
    );
}

#[test]
fn work_trees_live_in_the_sidecar_and_an_old_inline_file_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    let tree = snapshot(dir.path(), None, None).unwrap();
    let mut work = Work::new("ship", Policy::Single);
    work.base = Some(tree.clone());
    work.candidate = Some(Candidate {
        paths: vec!["a.txt".to_string()],
        snapshot: tree.clone(),
    });
    let path = dir.path().join("work.json");
    work.save(&path, true).unwrap();

    let main: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(
        main.get("base").is_none(),
        "work.json carries no file map: {main}"
    );
    assert!(main["candidate"].get("snapshot").is_none(), "{main}");
    assert!(path.with_extension("tree.json").is_file());
    let loaded = Work::load(&path).unwrap().unwrap();
    assert_eq!(loaded.base, Some(tree.clone()));
    assert_eq!(loaded.candidate.unwrap().snapshot, tree);

    let old = dir.path().join("old.json");
    let mut inline = serde_json::to_value(&work).unwrap();
    inline["base"] = serde_json::to_value(&tree).unwrap();
    inline["candidate"]["snapshot"] = serde_json::to_value(&tree).unwrap();
    std::fs::write(&old, serde_json::to_vec(&inline).unwrap()).unwrap();
    let legacy = Work::load(&old).unwrap().unwrap();
    assert_eq!(legacy.base, Some(tree.clone()));
    assert_eq!(legacy.candidate.unwrap().snapshot, tree);
}

#[test]
fn an_empty_tree_pair_writes_no_sidecar_and_clears_a_stale_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("work.json");
    let sidecar = path.with_extension("tree.json");
    let mut work = Work::new("ship", Policy::Single);
    work.save(&path, true).unwrap();
    assert!(!sidecar.exists(), "no file map means no sidecar write");
    work.base = Some(snapshot(dir.path(), None, None).unwrap());
    work.save(&path, true).unwrap();
    assert!(sidecar.is_file());
    work.base = None;
    work.save(&path, true).unwrap();
    assert!(
        !sidecar.exists(),
        "an emptied map removes the stale sidecar"
    );
}

#[test]
fn a_goal_stops_after_three_failed_checks() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    let shot = snapshot(dir.path(), None, None).unwrap();
    let mut work = Work::new("make the check pass", Policy::Goal {});
    work.plan = Some("plan".to_string());
    work.execute(shot.clone()).unwrap();
    for _ in 0..3 {
        work.verify(shot.clone()).unwrap();
        work.verdict(false, Decision::Automatic).unwrap();
    }
    assert_eq!(work.progress().exit_code(), Some(2));
}
