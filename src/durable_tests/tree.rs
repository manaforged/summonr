use std::path::Path;
use std::process::Command;

use crate::durable::tree::{changed, snapshot};

fn touch(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(path, text).expect("write");
}

#[test]
fn walk_reports_added_changed_and_removed_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    touch(&root.join("keep.txt"), "same");
    touch(&root.join("edit.txt"), "before");
    touch(&root.join("gone.txt"), "bye");
    touch(&root.join("target/debug/out"), "build output");
    touch(&root.join("sessions/x/session.jsonl"), "log");
    let sessions = root.join("sessions");
    let base = snapshot(root, Some(&sessions)).expect("base");

    touch(&root.join("edit.txt"), "after, longer");
    touch(&root.join("src/new.rs"), "fn main() {}");
    std::fs::remove_file(root.join("gone.txt")).expect("remove");
    touch(&root.join("target/debug/out"), "rebuilt");
    touch(&root.join("sessions/x/work.json"), "{}");
    let capture = snapshot(root, Some(&sessions)).expect("capture");

    assert_eq!(
        changed(&base, &capture),
        vec![
            "edit.txt".to_string(),
            "gone.txt".to_string(),
            "src/new.rs".to_string()
        ]
    );
    assert!(changed(&base, &base).is_empty());
}

#[test]
fn git_listing_honours_gitignore() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    touch(&root.join(".gitignore"), "ignored/\n");
    touch(&root.join("tracked.txt"), "one");
    git(&["add", "."]);
    let base = snapshot(root, None).expect("base");

    touch(&root.join("ignored/junk.txt"), "noise");
    touch(&root.join("untracked.txt"), "two");
    touch(&root.join("tracked.txt"), "one more");
    let capture = snapshot(root, None).expect("capture");

    assert_eq!(
        changed(&base, &capture),
        vec!["tracked.txt".to_string(), "untracked.txt".to_string()]
    );
}

#[test]
fn a_same_size_edit_with_the_old_timestamp_still_counts_as_a_change() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("a.txt");
    std::fs::write(&file, "aaaa").expect("write");
    let modified = std::fs::metadata(&file)
        .expect("meta")
        .modified()
        .expect("mtime");
    let before = snapshot(dir.path(), None).expect("snapshot");
    std::fs::write(&file, "bbbb").expect("rewrite");
    std::fs::File::options()
        .write(true)
        .open(&file)
        .expect("open")
        .set_modified(modified)
        .expect("set mtime");
    let after = snapshot(dir.path(), None).expect("snapshot");
    assert_eq!(changed(&before, &after), vec!["a.txt"]);
}
