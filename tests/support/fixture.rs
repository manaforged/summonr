use serde_json::{Value, json};
use std::collections::HashMap;
use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, Write};
use std::process;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

mod content;
mod tools;

use tools::{Tool, advertised, big_text, describe, run, text, wait};

const DEFAULT_PROTOCOL: &str = "2026-07-28";

#[derive(Clone)]
pub(super) struct Out(Arc<Mutex<io::Stdout>>);

impl Out {
    pub(super) fn send(&self, message: &Value) {
        let mut stdout = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if writeln!(stdout, "{message}")
            .and_then(|()| stdout.flush())
            .is_err()
        {
            process::exit(0);
        }
    }

    pub(super) fn result(&self, id: &Value, result: Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    fn split(&self, message: &Value, until: Instant) {
        let line = format!("{message}\n");
        let (head, rest) = line.split_at(line.len() / 2);
        let mut stdout = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        drop(
            stdout
                .write_all(head.as_bytes())
                .and_then(|()| stdout.flush()),
        );
        wait(until.saturating_duration_since(Instant::now()));
        drop(
            stdout
                .write_all(rest.as_bytes())
                .and_then(|()| stdout.flush()),
        );
    }

    fn error(&self, id: &Value, code: i64, message: &str) {
        self.send(
            &json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
        );
    }
}

fn append_file(var: &str) -> Option<File> {
    let path = env::var_os(var)?;
    OpenOptions::new().create(true).append(true).open(path).ok()
}

fn append_line(file: &mut Option<File>, line: &str) {
    if let Some(handle) = file {
        drop(writeln!(handle, "{line}").and_then(|()| handle.flush()));
    }
}

fn call(out: Out, id: Value, params: Value) {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let Some(row) = advertised().into_iter().find(|row| row.name == name) else {
        out.error(&id, -32602, &format!("unknown tool `{name}`"));
        return;
    };
    let tool = row.tool;
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let bytes =
        usize::try_from(args.get("bytes").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(0);
    match tool {
        Tool::Deaf => loop {
            thread::park();
        },
        Tool::Boom => return out.error(&id, -32000, &"e".repeat(bytes)),
        Tool::Split => {
            let pause = Duration::from_millis(args.get("ms").and_then(Value::as_u64).unwrap_or(0));
            let until = Instant::now() + pause;
            let message = json!({"jsonrpc": "2.0", "id": id, "result": text(big_text(bytes))});
            thread::spawn(move || out.split(&message, until));
            return;
        }
        Tool::Roots | Tool::Change | Tool::Progress | Tool::Log | Tool::Ask | Tool::Reprompt => {
            thread::spawn(move || tools::converse(tool, &out, &id, &params));
            return;
        }
        Tool::Echo
        | Tool::Write
        | Tool::Big
        | Tool::Slow
        | Tool::Hang
        | Tool::Image
        | Tool::Fail
        | Tool::Stderr
        | Tool::Exit
        | Tool::Leak => {}
    }
    thread::spawn(move || {
        if let Some(result) = run(tool, &args) {
            out.result(&id, result);
        }
    });
}

type Answers = (Mutex<HashMap<String, Value>>, Condvar);

fn answers() -> &'static Answers {
    static ANSWERS: OnceLock<Answers> = OnceLock::new();
    ANSWERS.get_or_init(|| (Mutex::new(HashMap::new()), Condvar::new()))
}

pub(super) fn answer(id: &str) -> Value {
    let (slots, ready) = answers();
    let slots = slots
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (mut slots, _) = ready
        .wait_timeout_while(slots, Duration::from_secs(5), |slots| {
            !slots.contains_key(id)
        })
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    slots.remove(id).unwrap_or(Value::Null)
}

fn handle(out: &Out, message: Value) {
    let Some(id) = message.get("id").cloned() else {
        return;
    };
    if message.get("method").is_none() {
        let (slots, ready) = answers();
        slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                id.to_string(),
                message.get("result").cloned().unwrap_or(message),
            );
        ready.notify_all();
        return;
    }
    if env::var_os("MCP_FIXTURE_MUTE").is_some() {
        return;
    }
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    match message.get("method").and_then(Value::as_str).unwrap_or("") {
        "initialize" => {
            let version = env::var("MCP_FIXTURE_VERSION").unwrap_or_else(|_unset| {
                params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_PROTOCOL)
                    .to_string()
            });
            out.result(
                &id,
                json!({
                    "protocolVersion": version,
                    "capabilities": content::capabilities(),
                    "serverInfo": {"name": "mcp-fixture", "version": "0"},
                }),
            );
        }
        "ping" => out.result(&id, json!({})),
        "tools/list" => {
            if env::var_os("MCP_FIXTURE_LOG_LIST").is_some() {
                out.send(&json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "info", "logger": "fixture", "data": "listing"}}));
            }
            let tools: Vec<Value> = advertised().into_iter().map(describe).collect();
            let mut page = json!({ "tools": tools });
            if env::var_os("MCP_FIXTURE_ENDLESS").is_some() {
                page["nextCursor"] = json!("more");
            }
            out.result(&id, page);
        }
        "tools/call" => call(out.clone(), id, params),
        method => match content::answer(method, &params).filter(|_| content::offered()) {
            Some(result) => out.result(&id, result),
            None => out.error(&id, -32601, "method not found"),
        },
    }
}

pub(crate) fn main() {
    let mut starts = append_file("MCP_FIXTURE_STARTS");
    append_line(&mut starts, &process::id().to_string());
    let mut log = append_file("MCP_FIXTURE_LOG");
    let out = Out(Arc::new(Mutex::new(io::stdout())));
    let read_pause = env::var("MCP_FIXTURE_READ_MS")
        .ok()
        .and_then(|ms| ms.parse().ok())
        .map(Duration::from_millis);
    let reader = thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            if let Some(pause) = read_pause {
                super::pause(pause);
            }
            append_line(&mut log, &line);
            if let Ok(message) = serde_json::from_str::<Value>(&line) {
                handle(&out, message);
            }
        }
    });
    drop(reader.join());
    while env::var_os("MCP_FIXTURE_LINGER").is_some() {
        thread::park();
    }
}
