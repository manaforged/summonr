#![cfg(unix)]
#![allow(
    dead_code,
    unused_imports,
    clippy::disallowed_methods,
    reason = "shares tests/support with other targets and polls a real terminal"
)]

#[path = "journey/delegation.rs"]
mod delegation;
mod support;
#[path = "support/terminal.rs"]
mod terminal;

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::{Model, Step};
use terminal::Terminal;

const HELD: Duration = Duration::from_secs(120);
const QUICK: Duration = Duration::from_secs(5);
const MAIL_TICK: Duration = Duration::from_secs(13);

struct Journey {
    home: tempfile::TempDir,
    model: Model,
    terminal: Terminal,
}

impl Journey {
    fn start(script: Vec<Step>) -> Self {
        Self::at(tempfile::tempdir().expect("journey home"), script)
    }

    fn at(home: tempfile::TempDir, script: Vec<Step>) -> Self {
        let model = Model::start(script);
        let terminal = Terminal::open(home.path(), 120, 40, model.port);
        terminal.wait("the splash", |screen| screen.contains("ask anything"));
        Self {
            home,
            model,
            terminal,
        }
    }

    fn say(&self, text: &str) {
        self.terminal.send(text.as_bytes());
        self.terminal.send(b"\r");
    }

    fn requests_reach(&self, count: usize, within: Duration) -> Instant {
        let deadline = Instant::now() + within;
        while self.model.requests().len() < count {
            assert!(
                Instant::now() < deadline,
                "{} of {count} model requests after {within:?}:\n{}",
                self.model.requests().len(),
                self.terminal.rows().join("\n")
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        Instant::now()
    }

    fn log(&self) -> Vec<Value> {
        let mut lines = Vec::new();
        for path in sessions(&self.home.path().join("sessions")) {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            lines.extend(
                text.lines()
                    .filter_map(|line| serde_json::from_str(line).ok()),
            );
        }
        lines
    }

    fn closed(&self, code: &str, within: Duration) {
        let deadline = Instant::now() + within;
        while !self
            .log()
            .iter()
            .any(|line| line["kind"] == "close" && line["code"] == code)
        {
            assert!(
                Instant::now() < deadline,
                "no {code} close after {within:?}:\n{}",
                self.terminal.rows().join("\n")
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn cancels(&self) -> usize {
        self.log()
            .iter()
            .filter(|line| line["kind"] == "close" && line["code"] == "cancelled")
            .count()
    }

    fn mail(&self, text: &str) {
        let status = Command::new(env!("CARGO_BIN_EXE_summonr"))
            .args(["mail", "send", "all", text])
            .current_dir(self.home.path())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.home.path())
            .env("SUMMONR_HOME", self.home.path())
            .status()
            .unwrap();
        assert!(status.success(), "mail send failed");
    }
}

fn sessions(root: &std::path::Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("session.jsonl"))
        .filter(|path| path.exists())
        .collect()
}

fn held() -> Step {
    Step::After(HELD, Box::new(Step::Text("too late")))
}

#[test]
fn escape_stops_a_waiting_turn_at_once() {
    let run = Journey::start(vec![held()]);
    run.say("work");
    run.requests_reach(1, QUICK);
    run.terminal.send(b"\x1b");
    run.closed("cancelled", QUICK);
}

#[test]
fn a_typed_message_reaches_the_model_during_a_reply() {
    let run = Journey::start(vec![held(), Step::Text("heard you")]);
    run.say("work");
    run.requests_reach(1, QUICK);
    run.say("use the other file");
    run.requests_reach(2, QUICK);
    let second = run.model.requests()[1].to_string();
    assert!(second.contains("use the other file"), "{second}");
    run.terminal
        .wait("the reply", |screen| screen.contains("heard you"));
    assert!(run.cancels() == 0, "a typed message cancelled the turn");
}

#[test]
fn peer_mail_wakes_an_idle_session() {
    let run = Journey::start(vec![Step::Text("first"), Step::Text("answered")]);
    run.say("work");
    run.terminal
        .wait("the reply", |screen| screen.contains("first"));
    run.mail("ping");
    run.requests_reach(2, MAIL_TICK);
}

#[test]
fn peer_mail_starts_no_turn_after_a_cancel() {
    let run = Journey::start(vec![held(), Step::Text("answered")]);
    run.say("work");
    run.requests_reach(1, QUICK);
    run.terminal.send(b"\x1b");
    run.closed("cancelled", QUICK);
    run.mail("ping");
    std::thread::sleep(MAIL_TICK);
    assert_eq!(
        run.model.requests().len(),
        1,
        "mail restarted a cancelled session"
    );
    run.say("carry on");
    run.requests_reach(2, QUICK);
}

#[test]
fn stop_ends_a_background_agent() {
    let run = Journey::start(vec![
        Step::Call(
            "task",
            json!({"access": "research", "assignment": "look around", "background": true}),
        ),
        held(),
        held(),
    ]);
    run.say("start an agent");
    run.requests_reach(3, Duration::from_secs(20));
    run.say("/stop");
    let deadline = Instant::now() + QUICK;
    loop {
        let log = run.log();
        let stopped = log
            .iter()
            .filter(|line| line["kind"] == "child")
            .any(|line| line["record"]["state"]["status"] != "running");
        if stopped {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the agent kept running:\n{}",
            run.terminal.rows().join("\n")
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_closed_terminal_leaves_no_crash() {
    let home = tempfile::tempdir().unwrap();
    let model = Model::start(vec![]);
    let code = terminal::hang_up(home.path(), model.port);
    assert!(code.is_some(), "kept running after the terminal closed");
    assert_ne!(code, Some(134), "aborted on a closed terminal");
    let logs: String = std::fs::read_dir(home.path().join("logs"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .collect();
    assert!(logs.contains("\"start\""), "no runtime log: {logs}");
    assert!(
        !logs.contains("\"panic\""),
        "panicked on a closed terminal: {logs}"
    );
}

#[test]
fn a_timed_out_command_keeps_running_as_a_job() {
    let run = Journey::start(vec![
        Step::Call(
            "run_command",
            json!({"command": "sleep 3; touch finished", "timeout_secs": 1}),
        ),
        Step::Text("noted"),
    ]);
    run.say("build it");
    run.requests_reach(2, Duration::from_secs(15));
    let result = run.model.requests()[1].to_string();
    assert!(result.contains("background job"), "{result}");
    let finished = run.home.path().join("finished");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !finished.exists() {
        assert!(
            Instant::now() < deadline,
            "the timed out command was killed"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn terminal_reports_during_a_turn_do_not_cancel_it() {
    let reports: [&[u8]; 16] = [
        b"\x1b[I",
        b"\x1b[O",
        b"\x1b[<35;10;10M",
        b"\x1b[200~pasted\x1b[201~",
        b"\x1b]11;rgb:0000/0000/0000\x1b\\",
        b"\x1b]11;rgb:0000/0000/0000\x07",
        b"\x1b[?62;22c",
        b"\x1b[>1;4000;13c",
        b"\x1b[12;1R",
        b"\x1b[?2026;2$y",
        b"\x1b[?1u",
        b"\x1b[4;800;1200t",
        b"\x1b[?4;2;800;600S",
        b"\x1b_Gi=31;OK\x1b\\",
        b"\x1bP>|ghostty 1.2\x1b\\",
        b"\x1bP1+r\x1b\\",
    ];
    let failed: Vec<String> = std::thread::scope(|scope| {
        let runs: Vec<_> = reports
            .iter()
            .map(|report| {
                scope.spawn(move || {
                    let run = Journey::start(vec![held()]);
                    run.say("work");
                    run.requests_reach(1, QUICK);
                    run.terminal.send(report);
                    std::thread::sleep(Duration::from_secs(1));
                    (run.cancels() > 0)
                        .then(|| String::from_utf8_lossy(report).escape_debug().to_string())
                })
            })
            .collect();
        runs.into_iter()
            .filter_map(|run| run.join().unwrap())
            .collect()
    });
    assert!(
        failed.is_empty(),
        "these reports cancelled the turn: {failed:?}"
    );
}

#[test]
#[ignore = "crossterm reads a lone escape as the Esc key; the fix belongs in the crossterm fork"]
fn a_report_split_across_reads_does_not_cancel_a_turn() {
    let run = Journey::start(vec![held()]);
    run.say("work");
    run.requests_reach(1, QUICK);
    for gap in [5, 20, 60] {
        run.terminal.send(b"\x1b");
        std::thread::sleep(Duration::from_millis(gap));
        run.terminal.send(b"[I");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            run.cancels(),
            0,
            "a focus report split by {gap} ms cancelled the turn"
        );
    }
}

#[test]
fn a_typed_message_leaves_a_running_command_alone() {
    let run = Journey::start(vec![
        Step::Call("run_command", json!({"command": "sleep 3; touch finished"})),
        Step::Text("saw your note"),
    ]);
    run.say("build");
    run.requests_reach(1, QUICK);
    std::thread::sleep(Duration::from_millis(500));
    run.say("also check the docs");
    run.requests_reach(2, Duration::from_secs(15));
    assert_eq!(run.cancels(), 0);
    let second = run.model.requests()[1].to_string();
    assert!(second.contains("also check the docs"), "{second}");
    let finished = run.home.path().join("finished");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !finished.exists() {
        assert!(Instant::now() < deadline, "the command was killed");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn escape_stops_a_running_command_at_once() {
    let run = Journey::start(vec![Step::Call(
        "run_command",
        json!({"command": "sleep 30; touch finished"}),
    )]);
    run.say("build");
    run.requests_reach(1, QUICK);
    std::thread::sleep(Duration::from_secs(1));
    run.terminal.send(b"\x1b");
    run.closed("cancelled", Duration::from_secs(2));
}

#[test]
fn a_rate_limit_retries_and_finishes_the_work() {
    let limit = r#"{"error":{"message":"rate limited","type":"rate_limit"}}"#;
    let run = Journey::start(vec![Step::Fail(429, "1", limit), Step::Text("recovered")]);
    run.say("work");
    run.terminal.wait("the reply after the limit", |screen| {
        screen.contains("recovered")
    });
}

#[test]
fn an_absurd_retry_wait_is_not_shown_to_the_user() {
    let limit = r#"{"error":{"message":"rate limited","type":"rate_limit"}}"#;
    let run = Journey::start(vec![
        Step::Fail(429, "911985", limit),
        Step::Fail(429, "911985", limit),
        Step::Text("recovered"),
    ]);
    run.say("work");
    std::thread::sleep(Duration::from_secs(5));
    let screen = run.terminal.rows().join("\n");
    assert!(!screen.contains("911985"), "{screen}");
}

#[test]
fn a_session_opens_and_quits_cleanly() {
    let run = Journey::start(vec![]);
    run.say("/quit");
    std::thread::sleep(Duration::from_secs(2));
    let logs: String = std::fs::read_dir(run.home.path().join("logs"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .collect();
    assert!(logs.contains("\"exit\""), "no exit record: {logs}");
    assert!(!logs.contains("\"panic\""), "{logs}");
}
