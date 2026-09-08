#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use super::{
    add, add_children, discard, finish, list, primary, reconcile_children, remove_child, status,
    subject, sweep,
};

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(repo)
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
    git(dir.path(), &["config", "core.autocrlf", "false"]);
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
        primary(&tree.path)
            .expect("primary")
            .canonicalize()
            .expect("canonical primary"),
        dir.path().canonicalize().expect("canonical")
    );
    assert_eq!(list(dir.path()).expect("list").len(), 1);
}

#[test]
fn child_worktrees_reconcile_disjoint_edits_without_commits() {
    let dir = repo();
    let trees = add_children(dir.path(), 3).expect("child worktrees");
    let container = trees[0].path.parent().expect("container").to_path_buf();
    std::fs::write(trees[0].path.join("one.txt"), "one\n").expect("one");
    std::fs::write(trees[1].path.join("two.txt"), "two\n").expect("two");
    std::fs::write(trees[2].path.join("three.bin"), [0_u8, 159, 255]).expect("three");
    let changes = vec![
        (trees[0].clone(), vec!["one.txt".to_string()]),
        (trees[1].clone(), vec!["two.txt".to_string()]),
        (trees[2].clone(), vec!["three.bin".to_string()]),
    ];

    let warnings = reconcile_children(dir.path(), &changes).expect("reconcile");

    assert!(warnings.is_empty());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("one.txt")).unwrap(),
        "one\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("two.txt")).unwrap(),
        "two\n"
    );
    assert_eq!(
        std::fs::read(dir.path().join("three.bin")).unwrap(),
        [0, 159, 255]
    );
    assert_eq!(
        Command::new("git")
            .args(["-C"])
            .arg(dir.path())
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .expect("rev-list")
            .stdout,
        b"1\n"
    );
    assert!(!container.exists(), "empty child container is removed");
}

#[test]
fn child_worktree_conflict_keeps_trees_and_parent_unchanged() {
    let dir = repo();
    let trees = add_children(dir.path(), 2).expect("child worktrees");
    std::fs::write(trees[0].path.join("README"), "first\n").expect("first");
    std::fs::write(trees[1].path.join("README"), "second\n").expect("second");
    let changes = vec![
        (trees[0].clone(), vec!["README".to_string()]),
        (trees[1].clone(), vec!["README".to_string()]),
    ];

    let error = reconcile_children(dir.path(), &changes).expect_err("conflict");

    assert!(error.to_string().contains("conflict"), "{error}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("README")).unwrap(),
        "root\n"
    );
    assert!(trees.iter().all(|tree| tree.path.exists()));
    for tree in &trees {
        remove_child(tree).expect("remove retained child");
    }
}

#[test]
fn parent_edit_while_children_run_keeps_the_child_worktree() {
    let dir = repo();
    let trees = add_children(dir.path(), 1).expect("child worktree");
    std::fs::write(trees[0].path.join("child.txt"), "child\n").expect("child edit");
    std::fs::write(dir.path().join("README"), "parent edit\n").expect("parent edit");
    let changes = vec![(trees[0].clone(), vec!["child.txt".to_string()])];

    let error = reconcile_children(dir.path(), &changes).expect_err("parent drift");

    assert!(
        error.to_string().contains("parent tracked files changed"),
        "{error}"
    );
    assert!(!dir.path().join("child.txt").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("README")).unwrap(),
        "parent edit\n"
    );
    assert!(trees[0].path.exists());
    remove_child(&trees[0]).expect("remove retained child");
}

#[test]
fn child_worktrees_refuse_tracked_parent_edits() {
    let dir = repo();
    std::fs::write(dir.path().join("README"), "dirty\n").expect("dirty");

    let error = add_children(dir.path(), 2).expect_err("dirty parent");

    assert!(error.to_string().contains("clean tracked files"), "{error}");
    assert!(list(dir.path()).expect("list").is_empty());
}

#[test]
fn finish_fast_forwards_main_and_removes_the_worktree() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    std::fs::write(tree.path.join("work.txt"), "from worktree\n").expect("edit");
    git(&tree.path, &["add", "work.txt"]);
    git(&tree.path, &["commit", "-m", "work"]);
    finish(&tree, "ship it", &["work.txt".to_string()]).expect("finish");
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
fn finish_commits_uncommitted_files_under_the_objective() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    std::fs::write(tree.path.join("dirty.txt"), "yes\n").expect("edit");
    finish(
        &tree,
        "Add the dirty file to the tree so the test can see it.",
        &["dirty.txt".to_string()],
    )
    .expect("finish");
    assert!(!tree.path.exists());
    assert!(dir.path().join("dirty.txt").is_file());
    let log = Command::new("git")
        .args(["-C"])
        .arg(dir.path())
        .args(["log", "-1", "--format=%s"])
        .output()
        .expect("git log");
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).trim(),
        "summon: add the dirty file to the tree so the test"
    );
}

#[test]
fn subject_fits_the_commit_line() {
    assert_eq!(
        subject("make cargo test pass."),
        "summon: make cargo test pass"
    );
    assert_eq!(subject("  Two\n  lines  "), "summon: two lines");
    assert!(subject(&"x".repeat(100)).chars().count() <= 50);
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

#[test]
fn finish_refuses_when_the_primary_moved_to_another_branch() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    std::fs::write(tree.path.join("work.txt"), "from worktree\n").expect("edit");
    git(dir.path(), &["checkout", "-q", "-b", "release"]);
    let error = finish(&tree, "ship it", &["work.txt".to_string()]).expect_err("wrong branch");
    assert!(error.to_string().contains("is on release"), "{error}");
    assert!(tree.path.exists());
    assert!(!dir.path().join("work.txt").exists());
}

#[test]
fn finish_keeps_a_tree_a_commit_hook_dirtied() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    let hooks = dir.path().join("hooks");
    std::fs::create_dir_all(&hooks).expect("hooks dir");
    git(
        dir.path(),
        &[
            "config",
            "core.hooksPath",
            hooks.to_str().expect("utf-8 path"),
        ],
    );
    let hook = hooks.join("post-commit");
    std::fs::write(&hook, "#!/bin/sh\necho generated > hook.txt\n").expect("hook");
    #[cfg(unix)]
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    std::fs::write(tree.path.join("work.txt"), "from worktree\n").expect("edit");
    let error =
        finish(&tree, "ship it", &["work.txt".to_string()]).expect_err("hook dirtied the tree");
    assert!(error.to_string().contains("left uncommitted"), "{error}");
    assert!(
        tree.path.join("hook.txt").exists(),
        "the hook's file survives"
    );
}

#[test]
fn finish_commits_only_the_touched_paths_and_names_the_rest() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    std::fs::write(tree.path.join("f.txt"), "from the run\n").expect("edit");
    std::fs::write(tree.path.join("junk.bin"), "stray\n").expect("stray");
    let error = finish(&tree, "add f", &["f.txt".to_string()]).expect_err("stray file");
    let text = error.to_string();
    assert!(
        text.contains("left uncommitted") && text.contains("junk.bin"),
        "{text}"
    );
    assert!(tree.path.exists(), "the tree is kept");
    let log = Command::new("git")
        .args(["-C"])
        .arg(&tree.path)
        .args(["show", "--stat", "--format=%s", "HEAD"])
        .output()
        .expect("git show");
    let shown = String::from_utf8_lossy(&log.stdout);
    assert!(
        shown.contains("summon: add f") && shown.contains("f.txt"),
        "{shown}"
    );
    assert!(!shown.contains("junk.bin"), "{shown}");
    std::fs::remove_file(tree.path.join("junk.bin")).expect("clean");
    let files = finish(&tree, "add f", &["f.txt".to_string()]).expect("finish");
    assert!(files.is_empty(), "nothing new to commit: {files:?}");
    assert!(dir.path().join("f.txt").is_file());
}

#[test]
fn finish_leaves_a_large_file_uncommitted() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    std::fs::write(tree.path.join("big.bin"), vec![0_u8; 6 * 1024 * 1024]).expect("big");
    std::fs::write(tree.path.join("f.txt"), "ok\n").expect("edit");
    let paths = ["big.bin".to_string(), "f.txt".to_string()];
    let error = finish(&tree, "add f", &paths).expect_err("big file");
    assert!(error.to_string().contains("big.bin"), "{error}");
}

#[test]
fn the_recorded_base_survives_a_branch_switch_in_the_primary() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    std::fs::write(tree.path.join("work.txt"), "from worktree\n").expect("edit");
    git(dir.path(), &["checkout", "-q", "-b", "release"]);
    let listed = list(dir.path()).expect("list");
    assert_eq!(
        listed[0].base, "main",
        "the base is the branch the run started on"
    );
    let error = finish(&listed[0], "ship it", &["work.txt".to_string()]).expect_err("wrong branch");
    assert!(error.to_string().contains("is on release"), "{error}");
    git(dir.path(), &["checkout", "-q", "main"]);
    finish(&listed[0], "ship it", &["work.txt".to_string()]).expect("finish on main");
    assert!(dir.path().join("work.txt").is_file());
}

#[test]
fn finish_refuses_a_large_file_the_model_committed_itself() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    std::fs::write(tree.path.join("big.bin"), vec![0_u8; 6 * 1024 * 1024]).expect("big");
    git(&tree.path, &["add", "big.bin"]);
    git(&tree.path, &["commit", "-q", "-m", "artifact"]);
    let error = finish(&tree, "ship it", &[]).expect_err("big file on the branch");
    assert!(error.to_string().contains("big.bin"), "{error}");
    assert!(tree.path.exists());
}

#[test]
fn a_touched_file_that_was_deleted_again_does_not_break_finish() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    std::fs::write(tree.path.join("f.txt"), "keep\n").expect("edit");
    let touched = ["f.txt".to_string(), "scratch.md".to_string()];
    finish(&tree, "add f", &touched).expect("a never-committed missing path is skipped");
    assert!(dir.path().join("f.txt").is_file());
}

#[test]
fn add_refuses_a_detached_primary() {
    let dir = repo();
    git(dir.path(), &["checkout", "-q", "--detach"]);
    let error = add(dir.path()).expect_err("detached");
    assert!(error.to_string().contains("detached HEAD"), "{error}");
}

#[test]
fn status_counts_commits_ahead_and_dirty_files_and_discard_guards_them() {
    let dir = repo();
    let tree = add(dir.path()).expect("add");
    let fresh = status(dir.path()).expect("status");
    assert_eq!(fresh.len(), 1);
    assert_eq!((fresh[0].ahead, fresh[0].dirty), (0, false));
    std::fs::write(tree.path.join("note.txt"), "hi\n").expect("write");
    assert!(status(dir.path()).expect("status")[0].dirty);
    let refused = discard(&tree).expect_err("dirty trees stay");
    assert!(refused.to_string().contains("uncommitted"), "{refused}");
    git(&tree.path, &["add", "note.txt"]);
    git(&tree.path, &["commit", "-q", "-m", "note"]);
    let committed = status(dir.path()).expect("status");
    assert_eq!((committed[0].ahead, committed[0].dirty), (1, false));
    let refused = discard(&tree).expect_err("unmerged commits stay");
    assert!(refused.to_string().contains("merge it first"), "{refused}");
    git(dir.path(), &["merge", "--ff-only", "--", &tree.branch]);
    discard(&tree).expect("a merged, clean tree goes");
    assert!(list(dir.path()).expect("list").is_empty());
}
