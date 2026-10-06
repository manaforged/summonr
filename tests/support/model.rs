use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::pause;

pub(crate) enum Step {
    Call(&'static str, Value),
    Text(&'static str),
    After(Duration, Box<Step>),
}

pub(crate) struct Model {
    pub(crate) port: u16,
    requests: Arc<Mutex<Vec<Value>>>,
}

fn kept_sizes(body: &Value) -> Vec<(String, u64)> {
    body.to_string()
        .split("[full output: ")
        .skip(1)
        .filter_map(|rest| {
            rest.split(']')
                .next()?
                .rsplit_once(" (")
                .map(|(path, _)| path)
        })
        .map(|path| path.replace("\\\\", "\\"))
        .map(|path| {
            let size = std::fs::metadata(&path).map_or(0, |meta| meta.len());
            (path, size)
        })
        .collect()
}

impl Model {
    pub(crate) fn start(script: Vec<Step>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let script = Arc::new(Mutex::new(script.into_iter().collect::<Vec<_>>()));
        let sink = Arc::clone(&requests);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let script = Arc::clone(&script);
                let sink = Arc::clone(&sink);
                thread::spawn(move || serve(stream, &script, &sink));
            }
        });
        Self { port, requests }
    }

    pub(crate) fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }

    pub(crate) fn tool_results(&self) -> Vec<String> {
        self.requests()
            .last()
            .and_then(|body| body.get("messages").and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .iter()
            .filter(|message| message["role"] == "tool")
            .map(|message| message["content"].to_string())
            .collect()
    }
}

fn serve(stream: TcpStream, script: &Mutex<Vec<Step>>, sink: &Mutex<Vec<Value>>) {
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
    let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
    let mut stream = stream;
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
    let mut body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    body["kept"] = json!(kept_sizes(&body));
    let index = {
        let mut requests = sink.lock().unwrap();
        requests.push(body);
        requests.len() - 1
    };
    let step = {
        let mut script = script.lock().unwrap();
        if script.is_empty() {
            Step::Text("done")
        } else {
            script.remove(0)
        }
    };
    let events = frames(index, step);
    reply(&mut stream, "text/event-stream", &events);
}

fn frames(index: usize, step: Step) -> String {
    let (delta, finish) = match step {
        Step::After(wait, step) => {
            pause(wait);
            return frames(index, *step);
        }
        Step::Call(name, arguments) => (
            json!({"role": "assistant", "tool_calls": [{
                "index": 0,
                "id": format!("call_{index}"),
                "type": "function",
                "function": {"name": name, "arguments": arguments.to_string()},
            }]}),
            "tool_calls",
        ),
        Step::Text(text) => (json!({"role": "assistant", "content": text}), "stop"),
    };
    let chunks = [
        json!({"id": "r", "model": "replay", "choices": [{"index": 0, "delta": delta, "finish_reason": null}]}),
        json!({"id": "r", "model": "replay", "choices": [{"index": 0, "delta": {}, "finish_reason": finish}]}),
        json!({"id": "r", "model": "replay", "choices": [], "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}}),
    ];
    let mut out: String = chunks
        .iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .collect();
    out.push_str("data: [DONE]\n\n");
    out
}

fn reply(stream: &mut TcpStream, kind: &str, body: &str) {
    drop(write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    ));
    drop(stream.flush());
}
