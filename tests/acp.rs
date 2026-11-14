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
        Self::with_flags(home, model, &[])
    }

    fn with_flags(home: &std::path::Path, model: &Model, flags: &[&str]) -> Self {
        Self::with_config(home, model, flags, "")
    }

    fn with_config(home: &std::path::Path, model: &Model, flags: &[&str], extra: &str) -> Self {
        std::fs::write(
            home.join("config.toml"),
            format!(
                "[endpoint.local]\nbase_url = \"http://127.0.0.1:{}/v1\"\nmodel = \"m\"\napi_key = \"k\"\ncontext_window = 32768\n{extra}",
                model.port
            ),
        )
        .unwrap();
        std::fs::write(home.join("policy.toml"), "ask = [\"echo granted\"]\n").unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_summonr"))
            .args(["-e", "local"])
            .args(flags)
            .arg("acp")
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

    fn call(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        self.until(method, |frame| frame["id"] == id)
    }

    fn initialize(&mut self) -> Value {
        self.call(1, "initialize", json!({"protocolVersion": 1}))
    }

    fn session(&mut self, cwd: &std::path::Path) -> String {
        self.initialize();
        let reply = self.call(2, "session/new", json!({"cwd": cwd, "mcpServers": []}));
        reply["result"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("session/new failed: {reply}"))
            .to_string()
    }

    fn prompt(&mut self, session: &str, text: &str) {
        self.send(
            &json!({"jsonrpc": "2.0", "id": 3, "method": "session/prompt", "params": {
                "sessionId": session,
                "prompt": [{"type": "text", "text": text}],
            }}),
        );
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
    assert!(
        ran(&model),
        "command did not run: {:?}",
        model.tool_results()
    );
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
    assert!(
        !ran(&model),
        "rejected command ran: {:?}",
        model.tool_results()
    );
}

#[cfg(unix)]
const ASK_HOOK: &str = r#"[hooks]
pre_tool = """printf '%s' '{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "ask", "permissionDecisionReason": "check"}}'"""
"#;

#[cfg(unix)]
#[test]
fn each_permission_request_names_its_own_tool_call_in_a_batch() {
    let home = tempfile::tempdir().expect("temp home");
    let model = Model::start(vec![
        Step::Calls(vec![
            ("read", json!({"path": "config.toml"})),
            ("read", json!({"path": "policy.toml"})),
        ]),
        Step::Text("done"),
    ]);
    let mut editor = Editor::with_config(home.path(), &model, &[], ASK_HOOK);
    let session = editor.session(home.path());
    editor.prompt(&session, "read both");
    let mut asked_for = Vec::new();
    for _ in 0..2 {
        let request = editor.until("session/request_permission", asked);
        asked_for.push((
            request["params"]["toolCall"]["toolCallId"].clone(),
            request["params"]["toolCall"]["title"].clone(),
        ));
        let answer = json!({"jsonrpc": "2.0", "id": request["id"], "result": {
            "outcome": {"outcome": "selected", "optionId": "allow_once"},
        }});
        editor.send(&answer);
    }
    reply(&mut editor);
    let announced: Vec<(Value, Value)> = editor
        .seen
        .iter()
        .filter(|frame| frame["params"]["update"]["sessionUpdate"] == "tool_call")
        .map(|frame| {
            let update = &frame["params"]["update"];
            (
                update["toolCallId"].clone(),
                update["rawInput"]["path"].clone(),
            )
        })
        .collect();
    assert_eq!(announced.len(), 2, "{announced:?}");
    for ((call, title), (id, path)) in asked_for.iter().zip(&announced) {
        assert_eq!(call, id, "asked {title} for the wrong call");
        let path = path.as_str().expect("read path");
        assert!(
            title.as_str().is_some_and(|title| title.contains(path)),
            "{title} does not name {path}"
        );
    }
}

fn reply(editor: &mut Editor) -> Value {
    editor.until("session/prompt reply", |frame| frame["id"] == 3)
}

#[test]
fn a_spent_token_budget_stops_with_max_tokens() {
    let home = tempfile::tempdir().expect("temp home");
    let model = Model::start(guarded_command());
    let mut editor = Editor::with_flags(home.path(), &model, &["--max-tokens", "1"]);
    let session = editor.session(home.path());
    editor.prompt(&session, "push it");
    let reply = loop {
        let frame = editor.until("prompt reply or permission", |frame| {
            asked(frame) || frame["id"] == 3
        });
        if !asked(&frame) {
            break frame;
        }
        let answer = json!({"jsonrpc": "2.0", "id": frame["id"], "result": {"outcome": {"outcome": "cancelled"}}});
        editor.send(&answer);
    };
    assert_eq!(reply["result"]["stopReason"], "max_tokens", "{reply}");
}

#[test]
fn a_turn_reports_context_usage() {
    let home = tempfile::tempdir().expect("temp home");
    let model = Model::start(vec![Step::Text("done")]);
    let mut editor = Editor::open(home.path(), &model);
    let session = editor.session(home.path());
    editor.prompt(&session, "hi");
    let usage = editor.until("usage_update", |frame| {
        frame["params"]["update"]["sessionUpdate"] == "usage_update"
    });
    assert_eq!(usage["params"]["update"]["size"], 32768, "{usage}");
    assert_eq!(usage["params"]["update"]["used"], 15, "{usage}");
    assert_eq!(reply(&mut editor)["result"]["stopReason"], "end_turn");
}

#[test]
fn a_request_during_a_prompt_is_busy_not_auth_required() {
    let home = tempfile::tempdir().expect("temp home");
    let model = Model::start(guarded_command());
    let mut editor = Editor::open(home.path(), &model);
    let session = editor.session(home.path());
    editor.prompt(&session, "push it");
    let request = editor.until("session/request_permission", asked);
    assert_eq!(
        request["params"]["toolCall"]["toolCallId"], "call_0",
        "{request}"
    );
    let busy = editor.call(
        9,
        "session/new",
        json!({"cwd": home.path(), "mcpServers": []}),
    );
    let code = busy["error"]["code"].as_i64().expect("an error code");
    assert!(
        code != -32000 && !(-32768..=-32000).contains(&code),
        "{busy}"
    );
    editor.send(&json!({"jsonrpc": "2.0", "id": request["id"], "result": {"outcome": {"outcome": "cancelled"}}}));
    assert_eq!(reply(&mut editor)["result"]["stopReason"], "end_turn");
}

#[test]
fn an_unknown_session_is_resource_not_found() {
    let home = tempfile::tempdir().expect("temp home");
    let model = Model::start(Vec::new());
    let mut editor = Editor::open(home.path(), &model);
    editor.initialize();
    let reply = editor.call(
        3,
        "session/prompt",
        json!({"sessionId": "nope", "prompt": []}),
    );
    assert_eq!(reply["error"]["code"], -32002, "{reply}");
}

#[test]
fn an_editor_sets_effort_through_config_options() {
    let home = tempfile::tempdir().expect("temp home");
    let model = Model::start(Vec::new());
    let mut editor = Editor::open(home.path(), &model);
    let session = editor.session(home.path());
    let set = json!({"sessionId": session, "configId": "effort", "value": "high"});
    let reply = editor.call(4, "session/set_config_option", set);
    assert_eq!(
        reply["result"]["configOptions"][0]["currentValue"], "high",
        "{reply}"
    );
    let bad = json!({"sessionId": session, "configId": "effort", "value": "loud"});
    let reply = editor.call(5, "session/set_config_option", bad);
    assert_eq!(reply["error"]["code"], -32602, "{reply}");
}

#[test]
fn initialize_advertises_only_stdio_mcp_and_unadvertised_transports_are_refused() {
    let home = tempfile::tempdir().expect("temp home");
    let model = Model::start(Vec::new());
    let mut editor = Editor::open(home.path(), &model);
    let init = editor.initialize();
    let caps = &init["result"]["agentCapabilities"];
    assert_eq!(
        caps["mcpCapabilities"],
        json!({"http": false, "sse": false}),
        "{init}"
    );
    assert_eq!(caps["loadSession"], false, "{init}");
    let http =
        json!({"type": "http", "name": "web", "url": "http://127.0.0.1:9/mcp", "headers": []});
    let reply = editor.call(
        2,
        "session/new",
        json!({"cwd": home.path(), "mcpServers": [http]}),
    );
    assert_eq!(reply["error"]["code"], -32602, "{reply}");
}
