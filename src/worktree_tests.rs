use std::path::Path;
use std::process::Command;

use super::{add, finish, list, primary, sweep};

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(args)
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "summon@test"]);
    git(dir.path(), &["config", "user.name", "Summoner"]);
    std::fs::write(dir.path().join("README"), "root\n").expect("write");
    git(dir.path(), &["add", "README"]);
    git(dir.path(), &["commit", "-m", "root"]);
    dir
}

#[test]
fn add_creates_a_linked_checkout_on_a_summon_branch() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    assert!(tree.path.is_dir());
    assert!(tree.branch.starts_with("summon/"));
    assert_eq!(tree.base, "main");
    assert_eq!(
        primary(&tree.path).expect("primary"),
        dir.path().canonicalize().expect("canonical")
    );
    assert_eq!(list(dir.path()).expect("list").len(), 1);
}

#[test]
fn finish_fast_forwards_main_and_removes_the_worktree() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    std::fs::write(tree.path.join("work.txt"), "from worktree\n").expect("edit");
    git(&tree.path, &["add", "work.txt"]);
    git(&tree.path, &["commit", "-m", "work"]);
    finish(&tree).expect("finish");
    assert!(!tree.path.exists());
    assert!(dir.path().join("work.txt").is_file());
    assert!(
        list(dir.path())
            .expect("list")
            .iter()
            .all(|item| !item.branch.starts_with("summon/"))
    );
}

#[test]
fn finish_refuses_uncommitted_files() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    std::fs::write(tree.path.join("dirty.txt"), "no\n").expect("edit");
    let error = finish(&tree).expect_err("dirty");
    assert!(error.to_string().contains("uncommitted"));
    assert!(tree.path.exists());
}

#[test]
fn sweep_keeps_a_live_worktree_at_head() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    let removed = sweep(dir.path()).expect("sweep");
    assert!(removed.is_empty());
    assert!(tree.path.exists());
    assert_eq!(list(dir.path()).expect("list").len(), 1);
}

#[test]
fn add_outside_git_is_a_configuration_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(!super::in_git(dir.path()));
    let error = add(dir.path()).expect_err("not git");
    assert!(error.to_string().contains("not a git repository"));
}
