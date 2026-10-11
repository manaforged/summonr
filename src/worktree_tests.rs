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
    git(&primary, &["config", "user.name", "Dev"]);
    git(&primary, &["config", "user.email", "dev@example.com"]);
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
    assert!(tree.unchanged().unwrap());
    std::fs::write(tree.path.join("note.txt"), "edit").unwrap();
    assert!(!tree.unchanged().unwrap());
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

#[test]
fn generated_files_from_two_children_do_not_block_their_edits() {
    let (_root, primary) = repo();
    let left = super::open_delta(&primary).unwrap();
    let right = super::open_delta(&primary).unwrap();
    for (tree, name, bytes) in [(&left, "left.py", "L"), (&right, "right.py", "R")] {
        std::fs::write(tree.path.join(name), bytes).unwrap();
        std::fs::create_dir_all(tree.path.join("__pycache__")).unwrap();
        std::fs::write(tree.path.join("__pycache__/m.cpython-312.pyc"), bytes).unwrap();
    }
    let landed = super::reconcile_children(
        &primary,
        &[(left.clone(), Vec::new()), (right.clone(), Vec::new())],
    )
    .unwrap();
    assert!(
        landed.iter().all(|landing| landing.kept.is_none()),
        "{:?}",
        landed
            .iter()
            .map(|landing| &landing.kept)
            .collect::<Vec<_>>()
    );
    assert_eq!(std::fs::read(primary.join("left.py")).unwrap(), b"L");
    assert_eq!(std::fs::read(primary.join("right.py")).unwrap(), b"R");
    assert!(!primary.join("__pycache__").exists());
}

#[test]
fn a_child_edit_to_a_non_ascii_name_lands() {
    let (_root, primary) = repo();
    std::fs::write(primary.join("café.txt"), "original").unwrap();
    git(&primary, &["add", "café.txt"]);
    git(&primary, &["commit", "-q", "-m", "cafe"]);
    let child = super::open_delta(&primary).unwrap();
    std::fs::write(child.path.join("café.txt"), "changed").unwrap();
    std::fs::write(child.path.join("naïve.txt"), "new").unwrap();
    let landed = super::reconcile_children(&primary, &[(child.clone(), Vec::new())]).unwrap();
    assert_eq!(std::fs::read(primary.join("café.txt")).unwrap(), b"changed");
    assert_eq!(std::fs::read(primary.join("naïve.txt")).unwrap(), b"new");
    assert_eq!(
        landed[0].paths,
        vec!["café.txt".to_string(), "naïve.txt".to_string()]
    );
    assert!(landed[0].kept.is_none(), "{:?}", landed[0].kept);
}

#[test]
fn a_worktree_run_commits_its_edits_without_caches_under_a_whole_word_subject() {
    let (_root, primary) = repo();
    let tree = super::add(&primary).unwrap();
    std::fs::write(
        tree.path.join("calc.py"),
        "def add(a, b):\n    return a + b\n",
    )
    .unwrap();
    std::fs::create_dir(tree.path.join("__pycache__")).unwrap();
    std::fs::write(
        tree.path.join("__pycache__/calc.cpython-312.pyc"),
        "bytecode",
    )
    .unwrap();
    let objective =
        "fix calc.py so python3 run_tests.py passes on every supported interpreter version";
    let committed = super::finish(
        &tree,
        objective,
        &[
            "calc.py".to_string(),
            "__pycache__/calc.cpython-312.pyc".to_string(),
        ],
    )
    .unwrap();
    assert_eq!(committed, vec!["calc.py".to_string()]);
    let files = Command::new("git")
        .args(["show", "--name-only", "--format=%s", "HEAD"])
        .current_dir(&primary)
        .output()
        .unwrap();
    let shown = String::from_utf8_lossy(&files.stdout);
    let subject = shown.lines().next().unwrap_or_default();
    assert!(!shown.contains("__pycache__"), "{shown}");
    assert!(subject.len() <= 72, "{subject}");
    let body = subject.trim_start_matches("summonr: ");
    assert!(
        objective.starts_with(body) && objective[body.len()..].starts_with(' '),
        "{subject}"
    );
}

fn tracked(primary: &Path, path: &str) {
    if let Some(dir) = Path::new(path).parent() {
        std::fs::create_dir_all(primary.join(dir)).unwrap();
    }
    std::fs::write(primary.join(path), "kept\n").unwrap();
    git(primary, &["add", "--", path]);
    git(primary, &["commit", "-q", "-m", "track"]);
}

fn on_main(primary: &Path) -> String {
    let out = Command::new("git")
        .args(["ls-tree", "-r", "-z", "--name-only", "main"])
        .current_dir(primary)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).replace('\0', "\n")
}

#[test]
fn a_worktree_run_lands_a_deleted_non_ascii_file() {
    let (_root, primary) = repo();
    tracked(&primary, "café.rs");
    let tree = super::add(&primary).unwrap();
    std::fs::remove_file(tree.path.join("café.rs")).unwrap();
    let committed = super::finish(&tree, "drop café", &["café.rs".to_string()]).unwrap();
    assert_eq!(committed, vec!["café.rs".to_string()]);
    assert!(!on_main(&primary).contains("café.rs"));
}

#[test]
fn a_glob_character_in_a_touched_path_stages_only_that_file() {
    let (_root, primary) = repo();
    let tree = super::add(&primary).unwrap();
    std::fs::write(tree.path.join("a[b].txt"), "touched\n").unwrap();
    std::fs::write(tree.path.join("ab.txt"), "not part of the run\n").unwrap();
    let result = super::finish(&tree, "edit a[b]", &["a[b].txt".to_string()]);
    assert!(result.is_err_and(|error| error.to_string().contains("ab.txt")));
    assert!(!on_main(&primary).contains("ab.txt"));
}

#[test]
fn a_rename_into_a_cache_path_is_not_thrown_away() {
    let (_root, primary) = repo();
    tracked(&primary, "a.c");
    let tree = super::add(&primary).unwrap();
    std::fs::create_dir(tree.path.join("__pycache__")).unwrap();
    git(&tree.path, &["mv", "a.c", "__pycache__/a.c"]);
    let result = super::finish(&tree, "move a.c", &[]);
    assert!(result.is_err_and(|error| error.to_string().contains("a.c")));
    assert!(tree.path.join("__pycache__/a.c").exists());
}
