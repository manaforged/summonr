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
    reader.serve(Cmd::Save("src/lib.rs".to_string(), "a\nb\n".to_string()));
    assert!(matches!(rx.try_recv(), Ok(Msg::System(text)) if text == "saved src/lib.rs · 2 lines"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/lib.rs")).expect("read"),
        "a\nb\n"
    );
    reader.serve(Cmd::Save("missing/dir/x".to_string(), String::new()));
    assert!(
        matches!(rx.try_recv(), Ok(Msg::System(text)) if text.starts_with("cannot save missing/dir/x"))
    );
}

#[test]
fn view_reads_the_file_and_its_added_lines() {
    let dir = repo();
    std::fs::write(dir.path().join("src/lib.rs"), "fn a() {}\nfn b() {}\n").expect("write");
    let (tx, mut rx) = unbounded_channel();
    let reader = Reader::new(dir.path().to_path_buf(), tx);
    reader.serve(Cmd::View(None, "src/lib.rs".to_string()));
    match rx.try_recv() {
        Ok(Msg::View { path, rope, added }) => {
            assert_eq!(path, "src/lib.rs");
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

// notify's FSEvents stream takes several seconds to start on macOS; the
// window below covers that under a loaded machine.
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
            changed = Some(paths);
        }
    }
    let paths = changed.unwrap_or_else(|| panic!("no change reported within 60s"));
    assert!(paths.contains(&"src/lib.rs".to_string()), "{paths:?}");
    assert!(
        !paths.iter().any(|path| path.starts_with("target/")),
        "{paths:?}"
    );
}
