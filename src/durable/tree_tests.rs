use std::time::{Duration, SystemTime};

use super::{changed, identity, mark, snapshot, walk};

fn backdate(path: &std::path::Path, by: Duration) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - by)
        .unwrap();
}

#[test]
fn a_same_size_rewrite_is_seen_even_when_it_restores_the_old_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let stale = root.join("stale.txt");
    let fresh = root.join("fresh.txt");
    std::fs::write(&stale, "aaaa").unwrap();
    std::fs::write(&fresh, "aaaa").unwrap();
    backdate(&stale, Duration::from_secs(20));
    backdate(&fresh, Duration::from_secs(20));
    let base = snapshot(root, &[], None).unwrap();

    std::fs::write(&stale, "bbbb").unwrap();
    backdate(&stale, Duration::from_secs(20));
    std::fs::write(&fresh, "bbbb").unwrap();
    let capture = snapshot(root, &[], Some(&base)).unwrap();

    assert_eq!(
        changed(&base, &capture),
        vec!["fresh.txt".to_string(), "stale.txt".to_string()],
        "a content write moves the change time, so a restored mtime still gets a rehash"
    );
    let again = snapshot(root, &[], Some(&capture)).unwrap();
    assert_eq!(
        changed(&capture, &again),
        Vec::<String>::new(),
        "untouched files reuse their marks"
    );
    assert_eq!(
        changed(&base, &snapshot(root, &[], None).unwrap()),
        vec!["fresh.txt".to_string(), "stale.txt".to_string()],
        "without a previous snapshot every file is hashed"
    );
}

#[test]
fn the_walk_stays_on_the_workspace_filesystem() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("mounted")).unwrap();
    std::fs::write(root.join("mounted/inside.txt"), "x").unwrap();
    std::fs::write(root.join("top.txt"), "x").unwrap();
    let here = super::device_of(&std::fs::metadata(root).unwrap());
    let mut same = Vec::new();
    walk(root, root, here, &mut same).unwrap();
    same.sort();
    assert_eq!(
        same,
        ["mounted/inside.txt", "top.txt"].map(std::path::PathBuf::from)
    );
    let mut other = Vec::new();
    walk(
        root,
        root,
        here.map(|device| device.wrapping_add(1)),
        &mut other,
    )
    .unwrap();
    assert_eq!(other, ["top.txt"].map(std::path::PathBuf::from));
}

#[test]
fn a_file_summonr_writes_its_own_output_to_is_not_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let events = dir.path().join("events.jsonl");
    let code = dir.path().join("a.txt");
    std::fs::write(&events, "{}").unwrap();
    std::fs::write(&code, "a").unwrap();
    let own = [identity(&std::fs::metadata(&events).unwrap()).unwrap()];
    assert!(mark(&events, "events.jsonl", None, &own).unwrap().is_none());
    assert!(mark(&code, "a.txt", None, &own).unwrap().is_some());
}

#[test]
fn bytecode_a_check_writes_does_not_change_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(git.success());
    std::fs::write(root.join("calc.py"), "def add(a, b):\n    return a + b\n").unwrap();
    let before = snapshot(root, &[], None).unwrap();
    std::fs::create_dir(root.join("__pycache__")).unwrap();
    std::fs::write(root.join("__pycache__/calc.cpython-312.pyc"), "bytecode").unwrap();
    let after = snapshot(root, &[], Some(&before)).unwrap();
    assert!(before == after, "a generated cache changed the snapshot");
}

#[test]
fn the_tree_cache_drops_workspaces_past_the_retention() {
    let cache = tempfile::tempdir().unwrap();
    let kept = cache.path().join("kept.json");
    let stale = cache.path().join("stale.json");
    std::fs::write(&kept, "{}").unwrap();
    std::fs::write(&stale, "{}").unwrap();
    let retain = std::time::Duration::from_secs(60);
    let later = std::time::SystemTime::now() + retain * 2;
    super::cache::prune(cache.path(), &kept, retain, later);
    assert!(kept.exists());
    assert!(!stale.exists());
}

#[test]
fn a_remembered_tree_reaches_a_new_process_through_the_cache() {
    let work = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("a.txt"), "one").unwrap();
    let mut first = snapshot(work.path(), &[], None).unwrap();
    super::cache::remember(cache.path(), work.path(), &mut first);
    let loaded = super::cache::remembered(cache.path(), work.path()).expect("cached");
    assert_eq!(loaded, first);
}
