#![cfg(unix)]

#[path = "score/hacks.rs"]
mod hacks;
#[path = "score/tasks.rs"]
mod tasks;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const JUDGE: &str = "You judge whether a coding objective is complete";

struct Workspace {
    home: tempfile::TempDir,
    work: tempfile::TempDir,
    files: tempfile::TempDir,
}

impl Workspace {
    fn new(check: &str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(work.path())
                .status()
                .unwrap()
                .success()
        );
        std::fs::create_dir_all(work.path().join(".summonr")).unwrap();
        std::fs::write(work.path().join(".summonr/check.toml"), check).unwrap();
        Self {
            home,
            work,
            files: tempfile::tempdir().unwrap(),
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_summonr"));
        command
            .args(args)
            .current_dir(self.work.path())
            .env("HOME", self.home.path())
            .env("SUMMONR_HOME", self.home.path())
            .stdin(Stdio::null());
        command
    }

    fn ledger(&self) -> Vec<Value> {
        std::fs::read_to_string(self.work.path().join(".git/summonr/score.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

struct Model {
    agent: Mutex<Vec<String>>,
    queued: Mutex<Vec<Value>>,
}

fn serve(listener: TcpListener, model: Arc<Model>) {
    for stream in listener.incoming().flatten() {
        let model = Arc::clone(&model);
        thread::spawn(move || answer(stream, &model));
    }
}

fn answer(stream: TcpStream, model: &Model) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut head = String::new();
    let mut length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
        if line == "\r\n" {
            break;
        }
        head.push_str(&line);
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let mut stream = stream;
    let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
    if path.ends_with("/registry.json") {
        return reply(&mut stream, "application/json", "{}");
    }
    if path.ends_with("/models") {
        return reply(
            &mut stream,
            "application/json",
            r#"{"data":[{"id":"replay","context_length":262144}]}"#,
        );
    }
    let text = String::from_utf8_lossy(&body).to_string();
    let (delta, finish) = if text.contains(JUDGE) {
        (
            json!({"role": "assistant", "content": r#"{"verdict": "done", "reason": "the agent says it is finished"}"#}),
            "stop",
        )
    } else {
        model.agent.lock().unwrap().push(text);
        let mut queued = model.queued.lock().unwrap();
        if queued.is_empty() {
            (
                json!({"role": "assistant", "content": "I finished the work."}),
                "stop",
            )
        } else {
            let delta = queued.remove(0);
            let finish = if delta.get("tool_calls").is_some() {
                "tool_calls"
            } else {
                "stop"
            };
            (delta, finish)
        }
    };
    let chunks = [
        json!({"id": "r", "model": "replay", "choices": [{"index": 0, "delta": delta, "finish_reason": null}]}),
        json!({"id": "r", "model": "replay", "choices": [{"index": 0, "delta": {}, "finish_reason": finish}]}),
        json!({"id": "r", "model": "replay", "choices": [], "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}}),
    ];
    let mut events: String = chunks
        .iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .collect();
    events.push_str("data: [DONE]\n\n");
    reply(&mut stream, "text/event-stream", &events);
}

fn reply(stream: &mut TcpStream, kind: &str, body: &str) {
    drop(write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    ));
    drop(stream.flush());
}

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    agent: Vec<String>,
    seconds: u64,
}

fn run_goal(workspace: &Workspace, objective: &str, limits: &str, queued: Vec<Value>) -> Run {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let model = Arc::new(Model {
        agent: Mutex::new(Vec::new()),
        queued: Mutex::new(queued),
    });
    let seen = Arc::clone(&model);
    thread::spawn(move || serve(listener, seen));
    std::fs::write(
        workspace.home.path().join("config.toml"),
        format!(
            "[endpoint.replay]\nbase_url = \"http://127.0.0.1:{port}/v1\"\nmodel = \"replay\"\napi_key = \"replay\"\ncontext_window = 262144\n\n[limits]\n{limits}"
        ),
    )
    .unwrap();
    let out = workspace.files.path().join("stdout");
    let err = workspace.files.path().join("stderr");
    let mut child = workspace
        .command(&[
            "-e",
            "replay",
            "--goal",
            objective,
            "--json",
            "--max-secs",
            "120",
        ])
        .env(
            "SUMMONR_MODELS_URL",
            format!("http://127.0.0.1:{port}/registry.json"),
        )
        .stdout(std::fs::File::create(&out).unwrap())
        .stderr(std::fs::File::create(&err).unwrap())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(150),
            "goal did not finish"
        );
        thread::park_timeout(Duration::from_millis(50));
    };
    Run {
        code: status.code(),
        stdout: std::fs::read_to_string(&out).unwrap(),
        stderr: std::fs::read_to_string(&err).unwrap(),
        agent: model.agent.lock().unwrap().clone(),
        seconds: started.elapsed().as_secs(),
    }
}

fn todo(items: Value) -> Value {
    json!({"role": "assistant", "tool_calls": [{"index": 0, "id": "call_todo", "type": "function",
        "function": {"name": "todo", "arguments": json!({"items": items}).to_string()}}]})
}

fn said(text: &str) -> Value {
    json!({"role": "assistant", "content": text})
}

fn ran(command: &str) -> Value {
    json!({"role": "assistant", "tool_calls": [{"index": 0, "id": "call_run", "type": "function",
        "function": {"name": "run_command", "arguments": json!({"command": command}).to_string()}}]})
}

fn counter(workspace: &Workspace, name: &str) -> String {
    let path = workspace.files.path().join(name);
    format!(
        "n=$(($(cat {path} 2>/dev/null || echo 0) + 1)); echo $n > {path}; echo $n",
        path = path.display()
    )
}

#[test]
fn a_goal_keeps_working_while_a_score_target_is_unmet_even_when_the_judge_says_done() {
    let workspace = Workspace::new("");
    let config = format!(
        "score = \"echo s $({})\"\n[targets]\ns = 2\n",
        counter(&workspace, "runs")
    );
    std::fs::write(workspace.work.path().join(".summonr/check.toml"), config).unwrap();
    let run = run_goal(&workspace, "raise s to 2", "", Vec::new());
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let ledger = workspace.ledger();
    assert_eq!(ledger.len(), 2, "{ledger:?}");
    assert_eq!(ledger[1]["scores"]["s"], 2.0);
    assert_eq!(run.agent.len(), 2, "agent turns: {}", run.agent.len());
    assert!(
        run.agent[1].contains("Unmet targets: s"),
        "{}",
        run.agent[1]
    );
}

#[test]
fn a_budget_breach_runs_halt_once_and_ends_the_goal() {
    let workspace = Workspace::new("");
    let halts = workspace.files.path().join("halts");
    let config = format!(
        "score = \"echo s 1\"\nspend = \"{spend}\"\nbudget = 2\nhalt = \"echo halted >> {halts}\"\n\n[targets]\ns = 2\n",
        spend = counter(&workspace, "spend").replace('"', "\\\""),
        halts = halts.display()
    );
    std::fs::write(workspace.work.path().join(".summonr/check.toml"), config).unwrap();
    let run = run_goal(&workspace, "raise s to 2", "", Vec::new());
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert_eq!(std::fs::read_to_string(&halts).unwrap(), "halted\n");
    assert!(
        run.stdout.contains("budget reached: $2 of $2"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_budget_breach_ends_a_goal_while_its_background_job_still_runs() {
    let workspace = Workspace::new("");
    let halts = workspace.files.path().join("halts");
    let config = format!(
        "score = \"echo s 1\"\nspend = \"{spend}\"\nbudget = 1\nhalt = \"echo halted >> {halts}\"\n\n[targets]\ns = 2\n",
        spend = counter(&workspace, "spend").replace('"', "\\\""),
        halts = halts.display()
    );
    std::fs::write(workspace.work.path().join(".summonr/check.toml"), config).unwrap();
    let train = json!({"role": "assistant", "tool_calls": [{"index": 0, "id": "call_train", "type": "function",
        "function": {"name": "run_command", "arguments": json!({"command": "sleep 60", "background": true}).to_string()}}]});
    let run = run_goal(
        &workspace,
        "finish the training run",
        "score_watch_secs = 0\njob_wait_secs = 2\n",
        vec![train],
    );
    assert_eq!(run.code, Some(2), "{}", run.stderr);
    assert!(run.seconds < 45, "waited {}s for the job", run.seconds);
    assert_eq!(std::fs::read_to_string(&halts).unwrap(), "halted\n");
    assert!(run.stdout.contains("killed job"), "{}", run.stdout);
}

#[test]
fn a_goal_ends_when_its_score_is_unavailable_for_three_turns() {
    let workspace = Workspace::new("");
    std::fs::write(
        workspace.work.path().join(".summonr/check.toml"),
        "score = \"echo not signed in >&2; exit 1\"\n\n[targets]\ns = 2\n",
    )
    .unwrap();
    let run = run_goal(&workspace, "raise s to 2", "", Vec::new());
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.stdout.contains("score unavailable for 3 turns"),
        "{}",
        run.stdout
    );
    assert_eq!(run.agent.len(), 3, "agent turns: {}", run.agent.len());
}

#[test]
fn a_run_that_rewrites_a_score_only_check_file_still_halts_at_the_budget_it_started_with() {
    let workspace = Workspace::new("");
    let halts = workspace.files.path().join("halts");
    let config = format!(
        "score = \"echo s 1\"\nspend = \"{spend}\"\nbudget = 2\nhalt = \"echo halted >> {halts}\"\n\n[targets]\ns = 2\n",
        spend = counter(&workspace, "spend").replace('"', "\\\""),
        halts = halts.display()
    );
    std::fs::write(workspace.work.path().join(".summonr/check.toml"), config).unwrap();
    let raise = json!({"role": "assistant", "tool_calls": [{"index": 0, "id": "call_raise", "type": "function",
        "function": {"name": "run_command", "arguments": json!({"command": "printf 'score = \"echo s 1\"\\nspend = \"echo 0\"\\nbudget = 1000\\n\\n[targets]\\ns = 2\\n' > .summonr/check.toml"}).to_string()}}]});
    let run = run_goal(&workspace, "raise s to 2", "", vec![raise]);
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert_eq!(std::fs::read_to_string(&halts).unwrap(), "halted\n");
    assert!(
        run.stdout.contains("budget reached: $2 of $2"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_goal_with_no_new_measurement_keeps_its_context_warns_at_three_turns_and_parks_at_six() {
    let workspace = Workspace::new("");
    std::fs::write(
        workspace.work.path().join(".summonr/check.toml"),
        "score = \"echo s 1\"\n\n[targets]\ns = 2\n",
    )
    .unwrap();
    let run = run_goal(&workspace, "raise s to 2", "", Vec::new());
    assert_eq!(run.code, Some(3), "{}", run.stderr);
    assert!(
        run.stdout.contains("no new measurement in 6 goal turns"),
        "{}",
        run.stdout
    );
    assert_eq!(run.agent.len(), 7, "agent turns: {}", run.agent.len());
    let answers = |index: usize| run.agent[index].matches("I finished the work.").count();
    assert!(answers(4) >= 4, "turn 5 should keep the earlier turns");
    assert!(
        run.agent[4].contains(
            "Goal turns since a new measurement: 3. The current approach is not producing evidence"
        ),
        "{}",
        run.agent[4]
    );
}

#[test]
fn a_worse_measurement_after_a_code_change_is_still_a_new_measurement_and_never_parks_the_goal() {
    let workspace = Workspace::new("");
    let path = workspace.files.path().join("n");
    let score = format!(
        "n=$(($(cat {path} 2>/dev/null || echo 0) + 1)); echo $n > {path}; set -- 9 8 7 6 5 4 3 100; eval v=\\\\${{$n}}; echo s $v",
        path = path.display()
    );
    std::fs::write(
        workspace.work.path().join(".summonr/check.toml"),
        format!("score = \"{score}\"\n\n[targets]\ns = 100\n"),
    )
    .unwrap();
    let turns = (1..=8)
        .flat_map(|turn| [ran(&format!("echo {turn} > work.txt")), said("measured")])
        .collect();
    let run = run_goal(&workspace, "raise s to 100", "", turns);
    assert_eq!(run.code, Some(0), "{}\n{}", run.stderr, run.stdout);
    assert_eq!(run.agent.len(), 16, "agent requests: {}", run.agent.len());
    assert!(!run.stdout.contains("no new measurement"), "{}", run.stdout);
}
