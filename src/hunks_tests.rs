use super::*;

const PATCH: &str = "diff --git a/src/app.rs b/src/app.rs\nindex 1111111..2222222 100644\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -10,3 +10,5 @@ fn main() {\n context\n-old\n+new\n+added\n tail\n@@ -40,2 +42,3 @@ fn other() {\n context\n-more\n+less\n";

#[test]
fn split_gives_each_hunk_a_self_contained_patch() {
    let hunks = split("src/app.rs", PATCH);
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[0].header, "@@ -10,3 +10,5 @@ fn main() {");
    assert_eq!(hunks[1].header, "@@ -40,2 +42,3 @@ fn other() {");
    for hunk in &hunks {
        assert!(
            hunk.patch.starts_with("diff --git a/src/app.rs"),
            "{}",
            hunk.patch
        );
        assert_eq!(
            hunk.patch
                .lines()
                .filter(|line| line.starts_with("@@ "))
                .count(),
            1
        );
    }
}

#[test]
fn hunk_ids_track_content_not_position() {
    let hunks = split("src/app.rs", PATCH);
    let shifted = PATCH.replace("@@ -10,3 +10,5 @@", "@@ -90,3 +90,5 @@");
    let moved = split("src/app.rs", &shifted);
    assert_eq!(hunks[0].id, moved[0].id);
    assert_ne!(hunks[0].id, hunks[1].id);
}

#[test]
fn marks_round_trip_and_prune() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert!(load(root).is_empty());
    let marks = apply(
        root,
        "src/app.rs",
        "aaaa",
        Op::Verdict(Some(Verdict::Accepted)),
    )
    .unwrap();
    assert_eq!(marks["src/app.rs"]["aaaa"].verdict, Some(Verdict::Accepted));
    let marks = apply(
        root,
        "src/app.rs",
        "aaaa",
        Op::Comment(Some("drop this".into())),
    )
    .unwrap();
    assert_eq!(
        marks["src/app.rs"]["aaaa"].comment.as_deref(),
        Some("drop this")
    );
    let marks = apply(root, "src/app.rs", "aaaa", Op::Verdict(None)).unwrap();
    assert_eq!(marks["src/app.rs"]["aaaa"].verdict, None);
    let marks = apply(root, "src/app.rs", "aaaa", Op::Comment(None)).unwrap();
    assert!(marks.is_empty(), "{marks:?}");
}

#[test]
fn drain_hands_comments_to_the_prompt_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert!(drain(root).is_none());
    apply(
        root,
        "src/app.rs",
        "bbbbccccdddd",
        Op::Comment(Some("this guard ignores empty input".into())),
    )
    .unwrap();
    let notes = drain(root).unwrap();
    assert!(notes.contains("src/app.rs"), "{notes}");
    assert!(notes.contains("this guard ignores empty input"), "{notes}");
    assert!(drain(root).is_none(), "comments deliver exactly once");
}

#[test]
fn reject_reverts_one_hunk_in_a_real_repo() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    committed(root, "one\ntwo\nthree\n");
    std::fs::write(root.join("f.txt"), "one\nTWO\nthree\n").unwrap();
    let patch = diff::patch(root, Some("f.txt")).unwrap();
    let hunks = split("f.txt", &patch);
    assert_eq!(hunks.len(), 1);
    apply(root, "f.txt", &hunks[0].id, Op::Reject).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("f.txt")).unwrap(),
        "one\ntwo\nthree\n"
    );
    let marks = load(root);
    assert_eq!(
        marks["f.txt"][&hunks[0].id].verdict,
        Some(Verdict::Rejected)
    );
}

#[test]
fn reject_reports_a_stale_hunk() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {:?}", output.stderr);
    };
    git(&["init", "-q"]);
    git(&["config", "core.autocrlf", "false"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    std::fs::write(root.join("f.txt"), "one\n").unwrap();
    git(&["add", "f.txt"]);
    git(&["commit", "-qm", "init"]);
    std::fs::write(root.join("f.txt"), "one\ntwo\n").unwrap();
    let error = apply(root, "f.txt", "deadbeef", Op::Reject).unwrap_err();
    assert!(error.to_string().contains("the hunk is gone"), "{error}");
}

#[test]
fn a_hunk_body_starts_at_its_hunk_header() {
    let patch = "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+new\n";
    let hunk = &super::split("a.txt", patch)[0];
    assert_eq!(hunk.body(), "@@ -1 +1 @@\n-old\n+new\n");
    assert!(hunk.patch.starts_with("diff --git"));
}

#[test]
fn hunk_ids_use_the_stable_content_hash() {
    let hunks = split("a.txt", "@@ -1 +1 @@\n-x\n+y\n");
    assert_eq!(hunks[0].id, crate::text::fnv64(b"a.txt\n-x\n+y"));
}

#[test]
fn review_notes_live_in_the_git_dir_not_the_working_tree() {
    let dir = tempfile::tempdir().unwrap();
    crate::diff::git_env(dir.path(), &[], &["init", "-q"]).unwrap();
    apply(
        dir.path(),
        "a.txt",
        "h1",
        Op::Comment(Some("check the edge".to_string())),
    )
    .unwrap();
    assert!(!dir.path().join(".summonr").exists());
    assert!(dir.path().join(".git/summonr/review.json").is_file());
    assert!(drain(dir.path()).is_some_and(|notes| notes.contains("check the edge")));
}

#[test]
fn a_reverted_hunk_reaches_the_next_turn_once() {
    let dir = tempfile::tempdir().unwrap();
    crate::diff::git_env(dir.path(), &[], &["init", "-q"]).unwrap();
    apply(
        dir.path(),
        "a.txt",
        "h1",
        Op::Verdict(Some(Verdict::Rejected)),
    )
    .unwrap();
    let first = drain(dir.path()).unwrap_or_default();
    assert!(
        first.contains("a.txt") && first.contains("reverted"),
        "{first}"
    );
    assert!(drain(dir.path()).is_none());
}

#[test]
fn review_marks_live_in_the_repository_state() {
    let plain = tempfile::tempdir().unwrap();
    assert_eq!(file(plain.path()), plain.path().join(".summonr").join(FILE));
    let repo = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let git = dunce::canonicalize(repo.path().join(".git")).unwrap();
    assert_eq!(file(repo.path()), git.join("summonr").join(FILE));
}

fn committed(root: &Path, text: &str) {
    for args in [
        &["init", "-q"][..],
        &["config", "core.autocrlf", "false"],
        &["config", "user.email", "t@t"],
        &["config", "user.name", "t"],
    ] {
        crate::diff::git_env(root, &[], args).unwrap();
    }
    std::fs::write(root.join("f.txt"), text).unwrap();
    crate::diff::git_env(root, &[], &["add", "f.txt"]).unwrap();
    crate::diff::git_env(root, &[], &["commit", "-qm", "init"]).unwrap();
}

#[test]
fn restore_puts_a_reverted_hunk_back_and_clears_its_mark() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    committed(root, "one\ntwo\nthree\n");
    std::fs::write(root.join("f.txt"), "one\nTWO\nthree\n").unwrap();
    let hunk = split("f.txt", &diff::patch(root, Some("f.txt")).unwrap())[0].clone();
    apply(root, "f.txt", &hunk.id, Op::Reject).unwrap();
    apply(root, "f.txt", &hunk.id, Op::Restore).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("f.txt")).unwrap(),
        "one\nTWO\nthree\n"
    );
    assert!(
        load(root)
            .get("f.txt")
            .and_then(|notes| notes.get(&hunk.id))
            .is_none_or(|note| note.verdict.is_none())
    );
    assert!(drain(root).is_none_or(|notes| !notes.contains("reverted")));
}

#[test]
fn reject_file_reverts_every_hunk_in_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let original: String = (1..=30).map(|line| format!("line {line}\n")).collect();
    committed(root, &original);
    let edited = original
        .replace("line 2\n", "LINE 2\n")
        .replace("line 28\n", "LINE 28\n");
    std::fs::write(root.join("f.txt"), &edited).unwrap();
    assert_eq!(
        split("f.txt", &diff::patch(root, Some("f.txt")).unwrap()).len(),
        2
    );
    apply(root, "f.txt", "", Op::RejectFile).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("f.txt")).unwrap(),
        original
    );
}

#[test]
fn a_hunk_that_ends_on_a_blank_line_reverts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let before = "x\ny\nz\n\nw\n";
    committed(root, before);
    std::fs::write(root.join("f.txt"), "X\ny\nz\n\nw\n").unwrap();
    let patch = diff::patch(root, Some("f.txt")).unwrap();
    assert!(patch.ends_with(" z\n \n"), "{patch:?}");
    let hunks = split("f.txt", &patch);
    apply(root, "f.txt", &hunks[0].id, Op::Reject).unwrap();
    assert_eq!(std::fs::read_to_string(root.join("f.txt")).unwrap(), before);
}

#[test]
fn a_new_file_keeps_its_trailing_whitespace_in_the_patch() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    committed(root, "one\n");
    std::fs::write(root.join("new.txt"), "text\n   \n").unwrap();
    let patch = diff::patch(root, Some("new.txt")).unwrap();
    assert!(patch.ends_with("+text\n+   \n"), "{patch:?}");
}
