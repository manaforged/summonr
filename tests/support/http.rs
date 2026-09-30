use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub(crate) enum Mode {
    Expire,
    HeldOpen,
}

pub(crate) struct Remote {
    pub(crate) port: u16,
}

#[derive(Default)]
struct State {
    generation: u32,
    current: Option<String>,
}

impl Remote {
    pub(crate) fn start(mode: Mode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(State::default()));
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = Arc::clone(&state);
                thread::spawn(move || serve(stream, mode, &state));
            }
        });
        Self { port }
    }

    pub(crate) fn config(&self, name: &str, extra: &str) -> String {
        format!(
            "[mcp.{name}]\nurl = \"http://127.0.0.1:{}/mcp\"\n{extra}\n",
            self.port
        )
    }
}

fn serve(stream: TcpStream, mode: Mode, state: &Mutex<State>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut length = 0;
    let mut session = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
        if let Some(value) = lower.strip_prefix("mcp-session-id:") {
            session = Some(value.trim().to_string());
        }
        if line == "\r\n" {
            break;
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let id = request["id"].clone();
    let mut stream = stream;
    let mut state = state.lock().unwrap();
    match request["method"].as_str().unwrap_or("") {
        "initialize" => {
            state.generation += 1;
            let fresh = format!("s{}", state.generation);
            state.current = Some(fresh.clone());
            let result = json!({"protocolVersion": "2025-03-26", "capabilities": {"tools": {}}, "serverInfo": {"name": "remote", "version": "0"}});
            reply(
                &mut stream,
                "200 OK",
                &[("mcp-session-id", &fresh)],
                &envelope(&id, result),
            );
        }
        "notifications/initialized" => reply(&mut stream, "202 Accepted", &[], ""),
        _ if session != state.current => reply(&mut stream, "404 Not Found", &[], "{}"),
        "tools/list" => {
            let tools = json!({"tools": [{"name": "echo", "description": "Echo.", "inputSchema": {"type": "object"}, "annotations": {"readOnlyHint": true}}]});
            reply(&mut stream, "200 OK", &[], &envelope(&id, tools));
        }
        _ => {
            let result = json!({"content": [{"type": "text", "text": "remote x"}]});
            match mode {
                Mode::Expire => {
                    state.current = None;
                    reply(&mut stream, "200 OK", &[], &envelope(&id, result));
                }
                Mode::HeldOpen => {
                    drop(state);
                    drop(write!(
                        stream,
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\ndata: {}\r\n\r\n",
                        envelope(&id, result)
                    ));
                    drop(stream.flush());
                    thread::park_timeout(Duration::from_secs(20));
                }
            }
        }
    }
}

fn envelope(id: &Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

fn reply(stream: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &str) {
    let extra: String = headers
        .iter()
        .map(|(key, value)| format!("{key}: {value}\r\n"))
        .collect();
    drop(write!(
        stream,
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n{body}",
        body.len()
    ));
    drop(stream.flush());
}
