use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use summonr_agent::hub::{Fact, Msg, Snapshot};

use super::harness::{Step, command, graded, settings, steps, typed};
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
    repository_with(scripts, &[])
}

fn repository_with(scripts: &str, files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("root");
    let repo = root.path().join("shop");
    let own = repo.join(".summonr");
    std::fs::create_dir_all(&own).expect("scripts folder");
    std::fs::write(own.join("scripts.toml"), scripts).expect("scripts file");
    for (name, text) in files {
        std::fs::write(repo.join(name), text).expect("fixture file");
    }
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
fn a_new_agent_pane_runs_setup_in_a_terminal_once_it_arrives() {
    if !in_window("scripts::a_new_agent_pane_runs_setup_in_a_terminal_once_it_arrives") {
        return;
    }
    let runtime = pumped();
    let (_root, repo) = repository("setup = 'echo set-up-$SUMMONR_WORKSPACE_NAME'\n");
    let folder = worktree(&repo, "three");
    let mut keys = vec![Step::Key(command("n"))];
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

fn filtered(query: &str, row: &'static str) -> Vec<Step> {
    let mut keys = vec![Step::Key(command("k"))];
    keys.extend(typed(query).into_iter().map(Step::Key));
    keys.push(Step::Click(row));
    keys
}

fn open_with(query: &str) -> Vec<Step> {
    let mut keys = vec![Step::Key(command("k"))];
    keys.extend(typed(query).into_iter().map(Step::Key));
    keys
}

fn alive(pid_file: &Path) -> bool {
    let pid = std::fs::read_to_string(pid_file).unwrap_or_default();
    Command::new("kill")
        .args(["-0", pid.trim()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn until_dead(pid_file: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while alive(pid_file) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    !alive(pid_file)
}

const TWO_ACTIONS: &str = "run_mode = 'nonconcurrent'\n\n\
[run.web]\ncommand = 'echo $$ > web.pid; echo web-up; exec sleep 60'\n\n\
[run.api]\ncommand = 'echo api-up; exec sleep 60'\ndefault = true\n";

#[test]
fn two_named_actions_list_two_rows_and_nonconcurrent_leaves_one_running() {
    if !in_window("scripts::two_named_actions_list_two_rows_and_nonconcurrent_leaves_one_running") {
        return;
    }
    let runtime = pumped();
    let (_root, repo) = repository(TWO_ACTIONS);
    let at = || {
        settings(Snapshot {
            workspace: repo.clone(),
            ..graded()
        })
    };
    let listed = steps(at(), open_with("run"), |_| ());
    let index = |label: &str| listed.texts.iter().position(|text| text == label);
    let (api, web) = (index("Run api"), index("Run web"));
    assert!(api.is_some() && web.is_some(), "{}", listed.text());
    assert!(
        api < web,
        "the default action is not first: {}",
        listed.text()
    );
    let mut keys = filtered("run web", "Run web");
    keys.push(Step::Wait(Box::new(move |app: &App| {
        screen_until(runtime, app, "web-up");
    })));
    keys.extend(filtered("run api", "Run api"));
    let pid = repo.join("web.pid");
    let seen = steps(at(), keys, |app| {
        let screen = screen_until(runtime, app, "api-up");
        let names: Vec<String> = app
            .window
            .terminals
            .list
            .peek()
            .iter()
            .map(|term| term.label())
            .collect();
        (names, screen, until_dead(&pid))
    });
    let painted = seen.text();
    let (names, screen, web_stopped) = seen.read;
    assert_eq!(names, ["Run api"], "{painted}");
    assert!(
        screen.iter().any(|line| line.trim() == "api-up"),
        "{screen:?}"
    );
    assert!(web_stopped, "Run web still runs after Run api started");
}

#[test]
fn a_cloud_only_action_is_marked_and_never_runs_here() {
    if !in_window("scripts::a_cloud_only_action_is_marked_and_never_runs_here") {
        return;
    }
    let _runtime = pumped();
    let (_root, repo) =
        repository("[run.docs]\ncommand = 'echo ran > docs-ran.txt'\navailable_in = ['cloud']\n");
    let at = || {
        settings(Snapshot {
            workspace: repo.clone(),
            ..graded()
        })
    };
    let listed = steps(at(), open_with("run docs"), |_| ());
    assert!(listed.shows(|text| text == "Run docs"), "{}", listed.text());
    assert!(
        listed.shows(|text| text == super::super::palette::CLOUD_ONLY),
        "{}",
        listed.text()
    );
    let seen = steps(at(), filtered("run docs", "Run docs"), |app| {
        std::thread::sleep(Duration::from_millis(300));
        (
            app.window.terminals.list.peek().len(),
            app.window.showing.peek(),
        )
    });
    assert_eq!(seen.read, (0, None), "{}", seen.text());
    assert!(
        !repo.join("docs-ran.txt").exists(),
        "the cloud-only action ran"
    );
    assert!(
        seen.shows(|text| text.contains("Run docs is cloud only")),
        "{}",
        seen.text()
    );
}

#[test]
fn a_new_agent_pane_gets_ignored_env_files_before_setup_reads_them() {
    if !in_window("scripts::a_new_agent_pane_gets_ignored_env_files_before_setup_reads_them") {
        return;
    }
    let runtime = pumped();
    let (_root, repo) = repository_with(
        "setup = 'cat .env.local .env.tracked; echo setup-done'\n",
        &[
            (".gitignore", ".env.local\n"),
            (".env.tracked", "TRACKED=committed\n"),
        ],
    );
    std::fs::write(repo.join(".env.local"), "SECRET=from-root\n").expect("ignored file");
    std::fs::write(repo.join(".env.tracked"), "TRACKED=root-edit\n").expect("tracked edit");
    let folder = worktree(&repo, "four");
    let arrived = folder.clone();
    let mut keys = vec![Step::Key(command("n"))];
    keys.push(Step::Wait(Box::new(move |app: &App| {
        app.update(Msg::Fact(Fact::Settings(Snapshot {
            workspace: arrived,
            ..graded()
        })));
    })));
    let seen = steps(
        settings(Snapshot {
            workspace: repo,
            ..graded()
        }),
        keys,
        |app| screen_until(runtime, app, "setup-done"),
    );
    let screen = seen.read;
    let shown = |wanted: &str| screen.iter().any(|line| line.trim() == wanted);
    assert!(shown("SECRET=from-root"), "{screen:?}");
    assert!(shown("TRACKED=committed"), "{screen:?}");
    assert!(
        !shown("TRACKED=root-edit"),
        "a tracked file was copied: {screen:?}"
    );
    let copied = std::fs::read_to_string(folder.join(".env.local")).expect("copied file");
    assert_eq!(copied, "SECRET=from-root\n");
}
