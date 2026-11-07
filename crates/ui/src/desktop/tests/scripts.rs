use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use summonr_agent::hub::{Fact, Msg, Snapshot};

use super::harness::{Step, command, graded, settings, steps};
use super::in_window;
use super::terminals::{Pump, pumped};
use crate::desktop::App;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .status()
        .expect("run fixture git");
    assert!(status.success(), "git {args:?}");
}

fn repository(scripts: &str) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("root");
    let repo = root.path().join("shop");
    let own = repo.join(".summonr");
    std::fs::create_dir_all(&own).expect("scripts folder");
    std::fs::write(own.join("scripts.toml"), scripts).expect("scripts file");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["config", "user.email", "test@example.invalid"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "fixture: scripts"]);
    (root, repo)
}

fn worktree(repo: &Path, name: &str) -> PathBuf {
    let path = repo.with_file_name(name);
    let target = path.to_str().expect("utf-8 path");
    git(repo, &["worktree", "add", "-q", "-b", name, target]);
    path
}

fn screen_until(runtime: Pump, app: &App, wanted: &str) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        runtime.borrow_mut().pump();
        let screen = app
            .window
            .terminals
            .list
            .peek()
            .last()
            .map(|term| term.session.lines.get())
            .unwrap_or_default();
        if screen.iter().any(|line| line.contains(wanted)) || Instant::now() > deadline {
            return screen;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn palette(action: &'static str) -> Vec<Step> {
    vec![Step::Key(command("k")), Step::Click(action)]
}

#[test]
fn run_on_two_worktrees_starts_two_terminals_on_different_ports() {
    if !in_window("scripts::run_on_two_worktrees_starts_two_terminals_on_different_ports") {
        return;
    }
    let runtime = pumped();
    let (_root, repo) = repository("run = 'echo port=$SUMMONR_PORT'\n");
    let ports: Vec<String> = ["one", "two"]
        .into_iter()
        .map(|name| {
            let folder = worktree(&repo, name);
            let seen = steps(
                settings(Snapshot {
                    workspace: folder,
                    ..graded()
                }),
                palette(super::super::terminal::RUN),
                |app| {
                    let screen = screen_until(runtime, app, "port=");
                    (app.window.showing.peek().is_some(), screen)
                },
            );
            let painted = seen.text();
            let (showing, screen) = seen.read;
            assert!(
                showing,
                "the pane does not show the Run terminal: {painted}"
            );
            screen
                .iter()
                .find_map(|line| line.trim().strip_prefix("port=").map(str::to_owned))
                .unwrap_or_else(|| panic!("no port in {screen:?}"))
        })
        .collect();
    assert_eq!(ports.len(), 2);
    assert!(
        ports.iter().all(|port| port.parse::<u16>().is_ok()),
        "{ports:?}"
    );
    assert_ne!(ports[0], ports[1], "{ports:?}");
}

#[test]
fn a_new_branch_pane_runs_setup_in_a_terminal_once_it_arrives() {
    if !in_window("scripts::a_new_branch_pane_runs_setup_in_a_terminal_once_it_arrives") {
        return;
    }
    let runtime = pumped();
    let (_root, repo) = repository("setup = 'echo set-up-$SUMMONR_WORKSPACE_NAME'\n");
    let folder = worktree(&repo, "three");
    let mut keys = palette("New agent on branch");
    keys.push(Step::Wait(Box::new(move |app: &App| {
        app.update(Msg::Fact(Fact::Settings(Snapshot {
            workspace: folder,
            ..graded()
        })));
    })));
    let seen = steps(
        settings(Snapshot {
            workspace: repo,
            ..graded()
        }),
        keys,
        |app| {
            let screen = screen_until(runtime, app, "set-up-three");
            let count = app.window.terminals.list.peek().len();
            (app.window.showing.peek(), count, screen)
        },
    );
    let painted = seen.text();
    let (showing, count, screen) = seen.read;
    assert_eq!(showing, None, "setup took the pane from the agent");
    assert_eq!(count, 1, "{painted}");
    assert!(
        screen.iter().any(|line| line.trim() == "set-up-three"),
        "{screen:?}"
    );
}
