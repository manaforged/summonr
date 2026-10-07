use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Dev")
        .env("GIT_AUTHOR_EMAIL", "dev@example.com")
        .env("GIT_COMMITTER_NAME", "Dev")
        .env("GIT_COMMITTER_EMAIL", "dev@example.com")
        .status()
        .unwrap();
    assert!(status.success(), "{args:?}");
}

fn repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let primary = root.path().join("repo");
    std::fs::create_dir(&primary).unwrap();
    git(&primary, &["init", "-q", "-b", "main"]);
    std::fs::create_dir(primary.join("target")).unwrap();
    std::fs::write(primary.join("target").join("marker"), "cache").unwrap();
    std::fs::write(primary.join("README"), "hi").unwrap();
    git(&primary, &["add", "README"]);
    git(&primary, &["commit", "-q", "-m", "init"]);
    (root, primary)
}

#[test]
fn an_implement_child_checks_out_beside_the_repo_not_as_a_git_worktree() {
    let (_root, primary) = repo();
    let tree = super::open_delta(&primary).unwrap();
    assert_eq!(
        tree.path.parent().unwrap().file_name().unwrap(),
        "repo-deltas"
    );
    let listed = crate::diff::git_env(&primary, &[], &["worktree", "list", "--porcelain"]).unwrap();
    assert!(!listed.contains(&*tree.path.to_string_lossy()), "{listed}");
}

#[test]
fn an_implement_child_shares_the_parent_build_cache() {
    let (_root, primary) = repo();
    let tree = super::open_delta(&primary).unwrap();
    let cache = tree.path.join("target");
    assert!(
        std::fs::symlink_metadata(&cache)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(cache.join("marker")).unwrap(), b"cache");
}

#[test]
fn a_listed_cache_path_is_shared_into_the_delta() {
    let (_root, primary) = repo();
    std::fs::create_dir_all(primary.join(".summonr")).unwrap();
    std::fs::create_dir_all(primary.join("vendor/cache")).unwrap();
    std::fs::write(primary.join("vendor/cache/keep"), "k").unwrap();
    std::fs::write(primary.join(".summonr/linked"), "vendor/cache\n").unwrap();
    let tree = super::open_delta(&primary).unwrap();
    let shared = tree.path.join("vendor/cache");
    assert!(
        std::fs::symlink_metadata(&shared)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(shared.join("keep")).unwrap(), b"k");
}

#[test]
fn an_untouched_delta_is_clean_until_the_child_edits() {
    let (_root, primary) = repo();
    let tree = super::open_delta(&primary).unwrap();
    assert!(super::unchanged(&tree).unwrap());
    std::fs::write(tree.path.join("note.txt"), "edit").unwrap();
    assert!(!super::unchanged(&tree).unwrap());
}

#[test]
fn a_dirty_parent_opens_a_clean_delta() {
    let (_root, primary) = repo();
    std::fs::write(primary.join("README"), "dirty").unwrap();
    std::fs::write(primary.join("note.txt"), "new").unwrap();
    let tree = super::open_delta(&primary).unwrap();
    let status = crate::diff::git_env(&tree.path, &[], &["status", "--porcelain"]).unwrap();
    assert!(status.is_empty(), "{status}");
}

#[test]
fn two_children_land_different_files_without_a_touched_list() {
    let (_root, primary) = repo();
    let left = super::open_delta(&primary).unwrap();
    let right = super::open_delta(&primary).unwrap();
    std::fs::write(left.path.join("left.txt"), "L").unwrap();
    std::fs::write(right.path.join("right.txt"), "R").unwrap();
    let landed = super::reconcile_children(
        &primary,
        &[(left.clone(), Vec::new()), (right.clone(), Vec::new())],
    )
    .unwrap();
    assert_eq!(std::fs::read(primary.join("left.txt")).unwrap(), b"L");
    assert_eq!(std::fs::read(primary.join("right.txt")).unwrap(), b"R");
    assert!(!left.path.exists());
    assert!(!right.path.exists());
    assert_eq!(landed[0].paths, vec!["left.txt".to_string()]);
    assert_eq!(landed[1].paths, vec!["right.txt".to_string()]);
    assert!(landed[0].kept.is_none() && landed[1].kept.is_none());
}

#[test]
fn overlapping_edits_keep_the_later_delta() {
    let (_root, primary) = repo();
    let left = super::open_delta(&primary).unwrap();
    let right = super::open_delta(&primary).unwrap();
    std::fs::write(left.path.join("README"), "left\n").unwrap();
    std::fs::write(right.path.join("README"), "right\n").unwrap();
    let landed = super::reconcile_children(
        &primary,
        &[(left.clone(), Vec::new()), (right.clone(), Vec::new())],
    )
    .unwrap();
    assert_eq!(std::fs::read(primary.join("README")).unwrap(), b"left\n");
    assert!(!left.path.exists());
    assert!(right.path.exists());
    assert!(landed[1].kept.is_some());
    assert_eq!(landed[0].paths, vec!["README".to_string()]);
}

#[test]
fn a_child_commit_on_the_delta_still_lands() {
    let (_root, primary) = repo();
    let tree = super::open_delta(&primary).unwrap();
    std::fs::write(tree.path.join("extra.txt"), "x").unwrap();
    git(&tree.path, &["add", "extra.txt"]);
    git(&tree.path, &["commit", "-q", "-m", "child"]);
    let landed = super::reconcile_children(&primary, &[(tree.clone(), Vec::new())]).unwrap();
    assert_eq!(std::fs::read(primary.join("extra.txt")).unwrap(), b"x");
    assert!(!tree.path.exists());
    assert!(landed[0].kept.is_none());
}

#[test]
fn a_delta_that_leaves_the_parent_base_is_kept() {
    let (_root, primary) = repo();
    std::fs::write(primary.join("README"), "second").unwrap();
    git(&primary, &["add", "README"]);
    git(&primary, &["commit", "-q", "-m", "second"]);
    let first = crate::diff::git_env(&primary, &[], &["rev-parse", "HEAD~1"]).unwrap();
    let tree = super::open_delta(&primary).unwrap();
    git(&tree.path, &["reset", "--hard", &first]);
    std::fs::write(tree.path.join("lost.txt"), "nope").unwrap();
    let landed = super::reconcile_children(&primary, &[(tree.clone(), Vec::new())]).unwrap();
    assert!(!primary.join("lost.txt").exists());
    assert_eq!(std::fs::read(tree.path.join("lost.txt")).unwrap(), b"nope");
    let kept = landed[0].kept.as_deref().unwrap();
    assert!(kept.contains("moved off the parent base"), "{kept}");
}
