#![allow(
    dead_code,
    unused_imports,
    reason = "shares tests/support with other targets"
)]

mod support;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::{Value, json};
use support::{Model, Step};

const WAIT: Duration = Duration::from_secs(60);

struct Editor {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Value>,
    seen: Vec<Value>,
}

impl Drop for Editor {
    fn drop(&mut self) {
        drop(self.child.kill());
    }
}

impl Editor {
    fn open(home: &std::path::Path, model: &Model) -> Self {
        std::fs::write(
            home.join("config.toml"),
            format!(
                "[endpoint.local]\nbase_url = \"http://127.0.0.1:{}/v1\"\nmodel = \"m\"\napi_key = \"k\"\ncontext_window = 32768\n",
                model.port
            ),
        )
        .unwrap();
        std::fs::write(home.join("policy.toml"), "ask = [\"echo granted\"]\n").unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_summonr"))
            .args(["-e", "local", "acp"])
            .current_dir(home)
            .env_clear()
            .env("HOME", home)
            .env("USERPROFILE", home)
            .env("SUMMONR_HOME", home)
            .env("SUMMONR_MODELS_URL", "http://127.0.0.1:9/registry.json")
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sink, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(value) = serde_json::from_str(&line) {
                    drop(sink.send(value));
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
            seen: Vec::new(),
        }
    }

    fn send(&mut self, frame: &Value) {
        writeln!(self.stdin, "{frame}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn until(&mut self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        loop {
            let Ok(frame) = self.lines.recv_timeout(WAIT) else {
                panic!("no {what} from summonr acp; saw {:#?}", self.seen);
            };
            self.seen.push(frame.clone());
            if done(&frame) {
                return frame;
            }
        }
    }

    fn session(&mut self, cwd: &std::path::Path) -> String {
        self.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": 1}}));
        self.until("initialize reply", |frame| frame["id"] == 1);
        self.send(&json!({"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {"cwd": cwd, "mcpServers": []}}));
        let reply = self.until("session/new reply", |frame| frame["id"] == 2);
        reply["result"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("session/new failed: {reply}"))
            .to_string()
    }

    fn prompt(&mut self, session: &str, text: &str) {
        self.send(&json!({"jsonrpc": "2.0", "id": 3, "method": "session/prompt", "params": {
            "sessionId": session,
            "prompt": [{"type": "text", "text": text}],
        }}));
    }
}

fn asked(frame: &Value) -> bool {
    frame["method"] == "session/request_permission"
}

fn guarded_command() -> Vec<Step> {
    vec![
        Step::Call("run_command", json!({"command": "echo granted"})),
        Step::Text("done"),
    ]
}

fn pick(editor: &mut Editor, kind: &str) {
    let request = editor.until("session/request_permission", asked);
    let option = request["params"]["options"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|option| option["kind"] == kind)
        .unwrap_or_else(|| panic!("no {kind} option in {request}"));
    let answer = json!({"jsonrpc": "2.0", "id": request["id"], "result": {
        "outcome": {"outcome": "selected", "optionId": option["optionId"]},
    }});
    editor.send(&answer);
}

fn ran(model: &Model) -> bool {
    model
        .tool_results()
        .iter()
        .any(|result| result.contains("granted") && !result.contains("echo granted"))
}

#[test]
fn an_editor_allows_an_asked_command_and_it_runs() {
    let home = tempfile::tempdir().unwrap();
    let model = Model::start(guarded_command());
    let mut editor = Editor::open(home.path(), &model);
    let session = editor.session(home.path());
    editor.prompt(&session, "push it");
    pick(&mut editor, "allow_once");
    let reply = editor.until("session/prompt reply", |frame| frame["id"] == 3);
    assert_eq!(reply["result"]["stopReason"], "end_turn", "{reply}");
    assert!(ran(&model), "command did not run: {:?}", model.tool_results());
}

#[test]
fn an_editor_rejects_an_asked_command_and_it_does_not_run() {
    let home = tempfile::tempdir().unwrap();
    let model = Model::start(guarded_command());
    let mut editor = Editor::open(home.path(), &model);
    let session = editor.session(home.path());
    editor.prompt(&session, "push it");
    pick(&mut editor, "reject_once");
    let reply = editor.until("session/prompt reply", |frame| frame["id"] == 3);
    assert_eq!(reply["result"]["stopReason"], "end_turn", "{reply}");
    assert!(!ran(&model), "rejected command ran: {:?}", model.tool_results());
}
