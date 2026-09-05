use std::process::Command;

use super::{Diff, Stat, edited, hunk_header, hunks, numstat_row, patch, snapshot, stats, turns};

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
