use std::process::Command;

use super::{
    Diff, Stat, edited, git, hunk_header, hunks, numstat_row, patch, snapshot, stats, turns,
};

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?}");
    };
    run(&["init", "-q", "-b", "main"]);
    run(&["config", "user.email", "t@example.invalid"]);
    run(&["config", "user.name", "t"]);
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").expect("write");
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "seed"]);
    dir
}

#[test]
fn numstat_rows_parse_and_binary_rows_count_zero() {
    assert_eq!(
        numstat_row("3\t1\tsrc/lib.rs"),
        Some(Stat {
            path: "src/lib.rs".to_string(),
            added: 3,
            removed: 1,
            new: false,
        })
    );
    assert_eq!(numstat_row("-\t-\timg.png").map(|s| s.added), Some(0));
    assert_eq!(numstat_row(""), None);
}

#[test]
fn stats_count_edits_and_untracked_files_against_head() {
    let dir = repo();
    std::fs::write(dir.path().join("a.txt"), "one\nthree\nfour\n").expect("write");
    std::fs::write(dir.path().join("b.txt"), "new\nfile\n").expect("write");
    let diff = stats(dir.path()).expect("stats");
    assert_eq!(diff.base, "HEAD");
    assert_eq!(
        diff.files,
        vec![
            Stat {
                path: "a.txt".to_string(),
                added: 2,
                removed: 1,
                new: false,
            },
            Stat {
                path: "b.txt".to_string(),
                added: 2,
                removed: 0,
                new: true,
            },
        ]
    );
    assert_eq!(diff.summary(), "2 files · +4 −1 · vs HEAD");
    let text = patch(dir.path(), None).expect("patch");
    assert!(text.contains("-two\n+three\n+four"), "{text}");
    assert!(
        text.contains("+++ b/b.txt"),
        "untracked files show as new: {text}"
    );
    let one = patch(dir.path(), Some("b.txt")).expect("patch");
    assert!(!one.contains("a.txt"), "{one}");
    assert!(one.contains("+new"), "{one}");
}

#[test]
fn untracked_counts_match_text_binary_and_empty_patches() {
    let dir = repo();
    let samples: [(&str, &[u8], u32); 4] = [
        ("empty.txt", b"", 0),
        ("single.txt", b"new", 1),
        ("two lines.txt", b"first\nlast", 2),
        ("binary.bin", b"\0one\ntwo\n", 0),
    ];
    for (path, bytes, _) in samples {
        std::fs::write(dir.path().join(path), bytes).expect("write sample");
    }
    let diff = stats(dir.path()).expect("untracked statistics");
    for (path, _, added) in samples {
        let stat = diff
            .files
            .iter()
            .find(|stat| stat.path == path)
            .expect("file");
        assert_eq!(
            (stat.added, stat.removed, stat.new),
            (added, 0, true),
            "{path}"
        );
    }
    assert_eq!(diff.summary(), "4 files · +3 −0 · vs HEAD");
    let text = patch(dir.path(), Some("single.txt")).expect("no-newline patch");
    assert!(
        text.contains("+new\n\\ No newline at end of file"),
        "{text}"
    );
    let text = patch(dir.path(), Some("binary.bin")).expect("binary patch");
    assert!(text.contains("Binary files"), "{text}");
    assert!(
        hunks(dir.path(), "binary.bin")
            .expect("binary hunks")
            .is_empty()
    );
    assert_eq!(
        hunks(dir.path(), "single.txt").expect("text hunks"),
        vec![(1, 1)]
    );
}

#[test]
fn untracked_comparisons_distinguish_changes_from_missing_files() {
    let dir = repo();
    let path = "single.txt";
    let args = ["diff", "--no-index", "--numstat", "--", "/dev/null", path];
    let error = git(dir.path(), &args).expect_err("missing file");
    assert!(error.to_string().contains("Could not access"), "{error}");
    std::fs::write(dir.path().join(path), "new").expect("write");
    let text = git(dir.path(), &args).expect("differences are a result");
    assert!(text.starts_with("1\t0\t"), "{text}");
}

#[test]
fn nested_repositories_remain_visible_without_entering_their_files() {
    let dir = repo();
    let nested = dir.path().join("nested");
    let status = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .arg(&nested)
        .status()
        .expect("nested repository");
    assert!(status.success());
    std::fs::write(nested.join("inside.txt"), "inside\n").expect("nested file");
    std::fs::write(dir.path().join("outside.txt"), "outside").expect("parent file");
    let diff = stats(dir.path()).expect("parent statistics");
    assert_eq!(diff.files.len(), 2);
    assert_eq!(diff.files[0].path, "nested/");
    assert_eq!(diff.files[0].added, 0);
    assert!(diff.files[0].new);
    assert_eq!(diff.files[1].added, 1);
    let text = patch(dir.path(), None).expect("parent patch");
    assert!(text.contains("+outside"), "{text}");
    assert!(!text.contains("inside.txt"), "{text}");
    let error = patch(dir.path(), Some("nested/")).expect_err("separate workspace");
    assert!(error.to_string().contains("separate workspace"), "{error}");
}

#[test]
fn a_clean_tree_has_no_files() {
    let dir = repo();
    let state = snapshot(dir.path());
    assert!(state.contains("(HEAD), worktree clean"), "{state}");
    assert_eq!(
        stats(dir.path()).expect("stats"),
        Diff {
            base: "HEAD".to_string(),
            files: Vec::new(),
        }
    );
    assert_eq!(patch(dir.path(), None).expect("patch"), "");
}

#[test]
fn initial_repositories_show_staged_and_untracked_content() {
    for format in ["sha1", "sha256"] {
        let dir = tempfile::tempdir().expect("tempdir");
        let status = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args([
                "init",
                "-q",
                "-b",
                "main",
                &format!("--object-format={format}"),
            ])
            .status()
            .expect("git init");
        assert!(status.success());
        let empty = stats(dir.path()).expect("initial stats");
        assert_eq!(empty.summary(), "0 files · +0 −0 · vs empty tree");
        assert_eq!(
            snapshot(dir.path()),
            "no commits (empty tree), worktree clean"
        );
        assert_eq!(patch(dir.path(), None).expect("empty patch"), "");

        std::fs::write(dir.path().join("staged.txt"), "staged\n").expect("write");
        let status = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["add", "staged.txt"])
            .status()
            .expect("git add");
        assert!(status.success());
        std::fs::write(dir.path().join("staged.txt"), "staged\nworking\n").expect("write");
        std::fs::write(dir.path().join("untracked.txt"), "untracked\n").expect("write");
        let diff = stats(dir.path()).expect("initial files");
        assert_eq!(diff.summary(), "2 files · +3 −0 · vs empty tree");
        assert_eq!(
            snapshot(dir.path()),
            "no commits (empty tree), 2 files dirty"
        );
        let text = patch(dir.path(), None).expect("initial patch");
        assert!(text.contains("+staged\n+working\n"), "{text}");
        assert!(text.contains("+untracked\n"), "{text}");
        let staged = patch(dir.path(), Some("staged.txt")).expect("staged patch");
        assert!(staged.contains("+working\n"), "{staged}");
        assert!(!staged.contains("untracked.txt"), "{staged}");
        assert_eq!(
            hunks(dir.path(), "staged.txt").expect("staged hunks"),
            vec![(1, 2)]
        );
        assert_eq!(
            hunks(dir.path(), "untracked.txt").expect("untracked hunks"),
            vec![(1, 1)]
        );
    }
}

#[test]
fn an_unreadable_commit_is_not_an_empty_repository() {
    let dir = repo();
    std::fs::write(
        dir.path().join(".git/refs/heads/main"),
        "f".repeat(40) + "\n",
    )
    .expect("break the fixture reference");
    assert!(
        stats(dir.path())
            .expect_err("unreadable commit")
            .to_string()
            .contains("git diff failed")
    );
    assert!(
        patch(dir.path(), None)
            .expect_err("unreadable commit")
            .to_string()
            .contains("git diff failed")
    );
}

#[test]
fn an_initial_worktree_keeps_its_recorded_comparison_base() {
    let dir = repo();
    let linked = dir.path().join("linked");
    let status = Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["worktree", "add", "-q", "-b", "summon/start"])
        .arg(&linked)
        .arg("HEAD")
        .status()
        .expect("git worktree");
    assert!(status.success());
    let status = Command::new("git")
        .arg("-C")
        .arg(&linked)
        .args(["symbolic-ref", "HEAD", "refs/heads/summon/unborn"])
        .status()
        .expect("initial branch");
    assert!(status.success());
    let records = dir.path().join(".git").join("summon");
    std::fs::create_dir(&records).expect("records");
    std::fs::write(records.join("unborn.base"), "main\n").expect("base record");
    let linked = linked.canonicalize().expect("worktree path");
    let state = snapshot(&linked);
    assert_eq!(state, "no commits (main), worktree clean");
    assert_eq!(stats(&linked).expect("worktree diff").base, "main");
    std::fs::write(linked.join("a.txt"), "one\nchanged\n").expect("edit");
    let diff = stats(&linked).expect("recorded diff");
    assert_eq!(diff.summary(), "1 file · +1 −1 · vs main");
}

#[test]
fn snapshot_names_dirty_and_non_git_workspaces() {
    let dir = repo();
    std::fs::write(dir.path().join("b.txt"), "new\n").expect("write");
    assert!(snapshot(dir.path()).ends_with("1 file dirty"));

    let plain = tempfile::tempdir().expect("tempdir");
    assert_eq!(snapshot(plain.path()), "non-Git workspace");
}

#[test]
fn hunk_headers_give_added_ranges() {
    assert_eq!(hunk_header("@@ -1 +1,2 @@"), Some((1, 2)));
    assert_eq!(hunk_header("@@ -3,2 +4 @@ fn main()"), Some((4, 1)));
    assert_eq!(
        hunk_header("@@ -3,2 +2,0 @@"),
        None,
        "a pure deletion adds nothing"
    );
    assert_eq!(hunk_header("+++ b/x"), None);
}

#[test]
fn hunks_cover_edits_and_whole_untracked_files() {
    let dir = repo();
    std::fs::write(dir.path().join("a.txt"), "one\nx\ntwo\ny\nz\n").expect("write");
    std::fs::write(dir.path().join("b.txt"), "new\nfile\n").expect("write");
    assert_eq!(
        hunks(dir.path(), "a.txt").expect("hunks"),
        vec![(2, 1), (4, 2)]
    );
    assert_eq!(hunks(dir.path(), "b.txt").expect("hunks"), vec![(1, 2)]);
}

#[test]
fn edited_paths_come_from_editing_tool_calls_only() {
    let calls = [
        ("read_file", r#"{"path":"a.rs"}"#),
        ("edit_file", r#"{"path":"src/b.rs","old":"x","new":"y"}"#),
        ("write_file", r#"{"path":"c.md","content":""}"#),
        ("edit_file", r#"{"path":"src/b.rs"}"#),
        ("run_command", r#"{"command":"touch d"}"#),
        ("write_file", "not json"),
    ];
    assert_eq!(
        edited(calls),
        vec!["src/b.rs".to_string(), "c.md".to_string()]
    );
}

#[test]
fn turns_collect_each_turns_edited_paths_from_the_journal() {
    use crate::durable::log::{Line, Usage};
    use crate::provider::{ToolCall, ToolFunction, ToolKind};
    let call = |name: &str, path: &str| ToolCall {
        continuation: None,
        id: "1".to_string(),
        kind: ToolKind::default(),
        function: ToolFunction {
            name: name.to_string(),
            arguments: format!(r#"{{"path":"{path}"}}"#),
        },
    };
    let assistant = |calls: Vec<ToolCall>| Line::Assistant {
        text: String::new(),
        reasoning: String::new(),
        tool_calls: calls,
        usage: Usage::default(),
        model: String::new(),
    };
    let user = |text: &str, start: bool| Line::User {
        text: text.to_string(),
        source: None,
        images: Vec::new(),
        start,
    };
    let lines = [
        user("one", true),
        assistant(vec![call("edit_file", "a.rs"), call("read_file", "z.rs")]),
        user("tool result", false),
        assistant(vec![call("edit_file", "a.rs")]),
        user("two", true),
        assistant(vec![call("write_file", "b.rs")]),
        user("three", true),
    ];
    let dir = tempfile::tempdir().expect("tempdir");
    let journal = dir.path().join("session.jsonl");
    let text: String = lines
        .iter()
        .map(|line| serde_json::to_string(line).expect("json") + "\n")
        .collect();
    std::fs::write(&journal, text).expect("write");
    assert_eq!(
        turns(&journal).expect("turns"),
        vec![
            vec!["a.rs".to_string()],
            vec!["b.rs".to_string()],
            Vec::new()
        ]
    );
}
