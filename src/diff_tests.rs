use super::*;
use crate::worktree::tests::git;

#[test]
fn a_folder_outside_git_snapshots_as_a_non_git_workspace() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(snapshot(dir.path()), "non-Git workspace");
}

#[test]
fn a_clean_repository_snapshots_as_a_clean_worktree() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    git(dir.path(), &["add", "a.txt"]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    let clean = snapshot(dir.path());
    assert!(clean.ends_with("worktree clean"), "{clean}");
    std::fs::write(dir.path().join("a.txt"), "b\n").unwrap();
    let dirty = snapshot(dir.path());
    assert!(dirty.ends_with("1 file dirty"), "{dirty}");
}

#[test]
fn a_linked_checkout_on_a_summonr_branch_is_listed() {
    let dir = tempfile::tempdir().unwrap();
    let common = dir.path().join("repo").join(crate::policy::git::GIT_DIR);
    let admin = common.join("worktrees").join("child");
    std::fs::create_dir_all(&admin).unwrap();
    std::fs::write(admin.join("HEAD"), "ref: refs/heads/summonr/child\n").unwrap();
    let linked = dir.path().join("child");
    std::fs::write(
        admin.join("gitdir"),
        format!("{}\n", linked.join(crate::policy::git::GIT_DIR).display()),
    )
    .unwrap();
    let layout = Layout {
        git_dir: common.clone(),
        common,
        top: dir.path().join("repo"),
    };
    assert_eq!(layout.checkouts(), vec![dir.path().join("repo"), linked]);
}
