use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::unbounded_channel;

use super::{Reader, files, unignored};
use crate::tui::{Cmd, Msg};

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
    std::fs::write(dir.path().join(".gitignore"), "target/\n").expect("write");
    std::fs::create_dir_all(dir.path().join("src")).expect("mkdir");
    std::fs::create_dir_all(dir.path().join("target")).expect("mkdir");
    std::fs::write(dir.path().join("src/lib.rs"), "fn a() {}\n").expect("write");
    std::fs::write(dir.path().join("target/out"), "x").expect("write");
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "seed"]);
    dir
}

#[test]
fn files_follow_gitignore_and_skip_hidden_entries() {
    let dir = repo();
    let mut listed = files(dir.path());
    listed.sort();
    assert_eq!(listed, vec!["src/lib.rs".to_string()]);
}

#[test]
fn diff_reports_failure_and_can_retry_a_readable_repository() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (tx, mut rx) = unbounded_channel();
    let reader = Reader::new(dir.path().to_path_buf(), tx);
    reader.serve(Cmd::Diff(None));
    match rx.try_recv().expect("diff response") {
        Msg::DiffFailed(error) => assert!(error.contains("not a git repository"), "{error}"),
        _ => panic!("expected a failed diff response"),
    }
    let status = Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["init", "-q", "-b", "main"])
        .status()
        .expect("git init");
    assert!(status.success());
    std::fs::write(dir.path().join("new.txt"), "new\n").expect("write");
    reader.serve(Cmd::Diff(None));
    match rx.try_recv().expect("retry response") {
        Msg::Diff(diff) => assert_eq!(diff.summary(), "1 file · +1 −0 · vs empty tree"),
        _ => panic!("expected the recovered diff"),
    }
}

#[test]
fn whole_patch_includes_tracked_and_untracked_changes() {
    let dir = repo();
    std::fs::write(dir.path().join("src/lib.rs"), "fn b() {}\n").expect("tracked edit");
    std::fs::write(dir.path().join("new.txt"), "new\n").expect("new file");
    let (tx, mut rx) = unbounded_channel();
    let reader = Reader::new(dir.path().to_path_buf(), tx);
    reader.serve(Cmd::Patch(None, String::new()));
    let Msg::Patch { path, result, .. } = rx.try_recv().expect("whole patch") else {
        panic!("expected patch response");
    };
    let text = result.expect("successful patch");
    assert!(path.is_empty());
    assert!(text.contains("-fn a() {}\n+fn b() {}"), "{text}");
    assert!(text.contains("+++ b/new.txt\n"), "{text}");
    assert!(text.contains("+new\n"), "{text}");
}

#[test]
fn patch_failure_preserves_scope_and_retry_returns_only_that_file() {
    let primary = repo();
    let dir = tempfile::tempdir().expect("unreadable repository");
    let (tx, mut rx) = unbounded_channel();
    let reader = Reader::new(primary.path().to_path_buf(), tx);
    reader.serve(Cmd::Patch(
        Some(dir.path().to_path_buf()),
        "new.txt".to_string(),
    ));
    match rx.try_recv().expect("failed patch") {
        Msg::Patch {
            root,
            path,
            result: Err(error),
        } => {
            assert_eq!(root.as_deref(), Some(dir.path()));
            assert_eq!(path, "new.txt");
            assert!(error.contains("not a git repository"), "{error}");
        }
        _ => panic!("expected a failed patch response"),
    }
    let status = Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["init", "-q", "-b", "main"])
        .status()
        .expect("git init");
    assert!(status.success());
    std::fs::write(dir.path().join("new.txt"), "selected\n").expect("selected file");
    std::fs::write(dir.path().join("other.txt"), "other\n").expect("other file");
    reader.serve(Cmd::Patch(
        Some(dir.path().to_path_buf()),
        "new.txt".to_string(),
    ));
    match rx.try_recv().expect("recovered patch") {
        Msg::Patch {
            root,
            path,
            result: Ok(text),
        } => {
            assert_eq!(root.as_deref(), Some(dir.path()));
            assert_eq!(path, "new.txt");
            assert!(text.contains("+selected\n"), "{text}");
            assert!(!text.contains("other.txt"), "{text}");
        }
        _ => panic!("expected the selected patch"),
    }
}

#[test]
fn unignored_drops_git_ignored_and_dot_paths() {
    let dir = repo();
    let kept = unignored(
        dir.path(),
        vec![
            "target/out".to_string(),
            ".git/index".to_string(),
            "src/lib.rs".to_string(),
            "src/lib.rs".to_string(),
        ],
    );
    assert_eq!(kept, vec!["src/lib.rs".to_string()]);
    assert_eq!(
        unignored(Path::new("/nonexistent"), vec!["a".to_string()]),
        vec!["a".to_string()]
    );
}

#[test]
fn save_writes_the_file_and_reports_the_line_count() {
    let dir = repo();
    let (tx, mut rx) = unbounded_channel();
    let reader = Reader::new(dir.path().to_path_buf(), tx);
    reader.serve(Cmd::Save(
        dir.path().join("src/lib.rs"),
        "a\nb\n".to_string(),
    ));
    assert!(
        matches!(rx.try_recv(), Ok(Msg::Saved { result: Ok(()), text, .. }) if text == "a\nb\n")
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/lib.rs")).expect("read"),
        "a\nb\n"
    );
    reader.serve(Cmd::Save(dir.path().join("missing/dir/x"), String::new()));
    assert!(matches!(
        rx.try_recv(),
        Ok(Msg::Saved { result: Err(_), .. })
    ));
}

#[test]
fn view_reads_the_file_and_its_added_lines() {
    let dir = repo();
    std::fs::write(dir.path().join("src/lib.rs"), "fn a() {}\nfn b() {}\n").expect("write");
    let (tx, mut rx) = unbounded_channel();
    let reader = Reader::new(dir.path().to_path_buf(), tx);
    reader.serve(Cmd::View(None, "src/lib.rs".to_string()));
    match rx.try_recv() {
        Ok(Msg::View {
            path,
            file,
            rope,
            added,
            image: _,
        }) => {
            assert_eq!(path, "src/lib.rs");
            assert_eq!(file, dir.path().join("src/lib.rs"));
            assert_eq!(rope.to_string_lossy(), "fn a() {}\nfn b() {}\n");
            assert_eq!(added, vec![(2, 1)]);
        }
        other => panic!("expected a view: {}", other.is_ok()),
    }
    reader.serve(Cmd::View(None, "missing.rs".to_string()));
    assert!(
        matches!(rx.try_recv(), Ok(Msg::System(text)) if text.starts_with("cannot view missing.rs"))
    );
}

#[tokio::test]
async fn saving_a_view_preserves_its_root() {
    let primary = repo();
    let secondary = repo();
    let (tx, mut rx) = unbounded_channel();
    let reader = Reader::new(primary.path().to_path_buf(), tx);
    reader
        .run(Cmd::View(
            Some(secondary.path().to_path_buf()),
            "src/lib.rs".to_string(),
        ))
        .await;
    let Msg::View { file, .. } = rx.recv().await.expect("view response") else {
        panic!("expected a view");
    };
    reader
        .run(Cmd::Save(file, "secondary edits".to_string()))
        .await;
    assert!(matches!(
        rx.recv().await,
        Some(Msg::Saved { result: Ok(()), .. })
    ));
    assert_eq!(
        std::fs::read_to_string(primary.path().join("src/lib.rs")).expect("primary"),
        "fn a() {}\n"
    );
    assert_eq!(
        std::fs::read_to_string(secondary.path().join("src/lib.rs")).expect("secondary"),
        "secondary edits"
    );
}

#[tokio::test]
async fn the_watch_reports_a_changed_file_once_the_burst_settles() {
    let dir = repo();
    let (tx, mut rx) = unbounded_channel();
    let reader = Reader::new(dir.path().to_path_buf(), tx);
    reader.watch();
    let start = Instant::now();
    let mut changed = None;
    while start.elapsed() < Duration::from_secs(60) && changed.is_none() {
        std::fs::write(
            dir.path().join("src/lib.rs"),
            format!("fn a() {{}} // {:?}\n", start.elapsed()),
        )
        .expect("write");
        std::fs::write(dir.path().join("target/out"), "y").expect("write");
        if let Ok(Some(Msg::Changed(paths))) =
            tokio::time::timeout(Duration::from_millis(700), rx.recv()).await
        {
            assert!(
                !paths.iter().any(|path| path.starts_with("target/")),
                "{paths:?}"
            );
            if paths.contains(&"src/lib.rs".to_string()) {
                changed = Some(paths);
            }
        }
    }
    let paths = changed.unwrap_or_else(|| panic!("no change reported within 60s"));
    assert!(paths.contains(&"src/lib.rs".to_string()), "{paths:?}");
    assert!(
        !paths.iter().any(|path| path.starts_with("target/")),
        "{paths:?}"
    );
}
