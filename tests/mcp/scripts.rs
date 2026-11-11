use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::support::{Model, Session, read, slow};

const BASE: u16 = 41000;
const BLOCK: u16 = 10;

fn conductor_setup(session: &Session, setup: &str) {
    let dir = session.work.path().join(".conductor");
    std::fs::create_dir_all(&dir).expect("create the Conductor settings folder");
    std::fs::write(
        dir.join("settings.toml"),
        format!("[scripts]\nsetup = '{setup}'\n"),
    )
    .expect("write the Conductor settings");
}

fn commit_all(work: &Path) {
    for args in [
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["config", "core.autocrlf", "false"],
        vec!["add", "."],
        vec!["commit", "-qm", "fixture: record initial files"],
    ] {
        let status = Command::new("git")
            .args(args)
            .current_dir(work)
            .status()
            .expect("run fixture git");
        assert!(status.success());
    }
}

fn worktree_ports(session: &Session) -> Vec<u16> {
    let (ok, text) = session.cli(&["worktrees", "--json"]);
    assert!(ok, "{text}");
    let rows: Vec<Value> = serde_json::from_str(&text).expect("worktree JSON");
    rows.iter()
        .map(|row| {
            let path = Path::new(row["path"].as_str().expect("a worktree path"));
            let text = read(&path.join("port.txt"));
            text.trim()
                .parse()
                .unwrap_or_else(|_| panic!("port.txt in {}: {text:?}", path.display()))
        })
        .collect()
}

pub(crate) fn each_new_worktree_runs_setup_on_its_own_port_block() {
    let budget = slow(2).to_string();
    let session = Session::new();
    conductor_setup(&session, "echo $CONDUCTOR_PORT > port.txt");
    commit_all(session.work.path());
    for _ in 0..2 {
        let model = Model::start(vec![]);
        let flags = [
            "--worktree",
            "--goal",
            "finish",
            "--max-secs",
            &budget,
            "--json",
        ];
        let result = session.run(&model, "", &flags);
        assert_eq!(result.code, Some(3), "{}", read(&session.file("stderr")));
    }
    let ports = worktree_ports(&session);
    assert_eq!(ports.len(), 2, "{ports:?}");
    assert_ne!(ports[0], ports[1], "{ports:?}");
    for port in &ports {
        assert!(
            *port >= BASE && (port - BASE).is_multiple_of(BLOCK),
            "{port} starts no block: {ports:?}"
        );
    }
}

pub(crate) fn a_failed_setup_stops_the_run_naming_the_script_and_its_code() {
    let session = Session::new();
    conductor_setup(&session, "echo setup-says-hi; exit 3");
    commit_all(session.work.path());
    let model = Model::start(vec![]);
    let result = session.run(&model, "", &["--worktree", "--json"]);
    let stderr = read(&session.file("stderr"));
    assert_eq!(result.code, Some(1), "{stderr}");
    assert!(
        stderr.contains("setup script `echo setup-says-hi; exit 3` exited with code 3"),
        "{stderr}"
    );
    assert!(stderr.contains("setup-says-hi\n"), "{stderr}");
    let events: Vec<Value> = result
        .stdout
        .lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|_| panic!("not JSON on stdout: {line}"))
        })
        .collect();
    assert!(
        events.iter().any(|event| event["type"] == "aborted"
            && event["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("exited with code 3"))),
        "{}",
        result.stdout
    );
    assert!(
        model.requests().is_empty(),
        "the model was asked after setup failed"
    );
}

pub(crate) fn a_gitignored_env_file_reaches_a_new_worktree_before_setup_reads_it() {
    let budget = slow(2).to_string();
    let session = Session::new();
    let work = session.work.path();
    std::fs::write(work.join(".gitignore"), ".env.local\n").expect("write the ignore file");
    conductor_setup(&session, "cat .env.local > seen.txt");
    commit_all(work);
    std::fs::write(work.join(".env.local"), "SECRET=from-root\n").expect("write the ignored file");
    let model = Model::start(vec![]);
    let flags = [
        "--worktree",
        "--goal",
        "finish",
        "--max-secs",
        &budget,
        "--json",
    ];
    let result = session.run(&model, "", &flags);
    assert_eq!(result.code, Some(3), "{}", read(&session.file("stderr")));
    let (ok, text) = session.cli(&["worktrees", "--json"]);
    assert!(ok, "{text}");
    let rows: Vec<Value> = serde_json::from_str(&text).expect("worktree JSON");
    let path = Path::new(rows[0]["path"].as_str().expect("a worktree path"));
    assert_eq!(read(&path.join("seen.txt")), "SECRET=from-root\n");
}
