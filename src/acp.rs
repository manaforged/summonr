use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines, Stdin};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::endpoint::{self, Endpoint};
use crate::error::{Error, Result};
use crate::provider::Client;
use crate::rpc::{self, Frame};
use crate::runtime::Loop;
use crate::tools::Event;

type Input = Lines<BufReader<Stdin>>;
type Reply = std::result::Result<Value, (i64, String)>;

struct Server<F> {
    endpoint: Endpoint,
    build: F,
    out: UnboundedSender<Value>,
    current: Arc<Mutex<String>>,
    sessions: BTreeMap<String, Loop<Client>>,
}

pub async fn serve<F>(name: Option<&str>, model: Option<&str>, build: F) -> Result<i32>
where
    F: FnMut(&Endpoint, Client, &Path) -> Result<Loop<Client>>,
{
    let (out, mut queue) = unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(frame) = queue.recv().await {
            let line = format!("{frame}\n");
            if stdout.write_all(line.as_bytes()).await.is_err() || stdout.flush().await.is_err() {
                break;
            }
        }
    });
    let mut server = Server {
        endpoint: endpoint::resolve(name, model)?,
        build,
        out,
        current: Arc::new(Mutex::new(String::new())),
        sessions: BTreeMap::new(),
    };
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = input.next_line().await? {
        server.handle(&line, &mut input).await;
    }
    drop(server);
    drop(writer.await);
    Ok(0)
}

impl<F> Server<F>
where
    F: FnMut(&Endpoint, Client, &Path) -> Result<Loop<Client>>,
{
    async fn handle(&mut self, line: &str, input: &mut Input) {
        let Ok(Frame::Request { id, method, params }) = rpc::parse(line) else {
            return;
        };
        let reply = match method.as_str() {
            "initialize" => Ok(json!({
                "protocolVersion": 1,
                "agentCapabilities": {"loadSession": false},
                "authMethods": []
            })),
            "authenticate" => Ok(json!({})),
            "session/new" => self.open(&params),
            "session/prompt" => self.prompt(&params, input).await,
            other => Err((-32601, format!("{other} is not supported"))),
        };
        drop(self.out.send(answer(&id, reply)));
    }

    fn connect(&self, endpoint: &Endpoint) -> Result<Client> {
        Ok(Client::connect(endpoint)?
            .on_delta(self.stream("agent_message_chunk"))
            .on_reasoning(self.stream("agent_thought_chunk")))
    }

    fn open(&mut self, params: &Value) -> Reply {
        let cwd = params
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| (-32602, "session/new needs cwd".to_string()))?;
        let client = self.connect(&self.endpoint).map_err(failure)?;
        let mut session = (self.build)(&self.endpoint, client, &cwd).map_err(failure)?;
        let (fallbacks, skipped) =
            endpoint::fallbacks(&self.endpoint, |fallback| self.connect(fallback));
        for note in skipped {
            eprintln!("summonr: {note}");
        }
        session.set_fallbacks(fallbacks);
        let id = session.session_id();
        self.sessions.insert(id.clone(), session);
        Ok(json!({"sessionId": id}))
    }

    fn stream(&self, kind: &'static str) -> impl Fn(&str) + Send + Sync + 'static {
        let out = self.out.clone();
        let current = self.current.clone();
        move |text: &str| {
            let session = current.lock().map(|id| id.clone()).unwrap_or_default();
            let content = json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}});
            drop(out.send(update(&session, content)));
        }
    }

    async fn prompt(&mut self, params: &Value, input: &mut Input) -> Reply {
        let session = params
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let text = params
            .get("prompt")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(block)
            .collect::<Vec<_>>()
            .join("\n");
        if let Ok(mut current) = self.current.lock() {
            current.clone_from(&session);
        }
        let out = self.out.clone();
        let Some(agent) = self.sessions.get_mut(&session) else {
            return Err((-32602, format!("unknown session {session}")));
        };
        let outcome = {
            let tag = session.clone();
            let sink = out.clone();
            let turn = agent.turn_with(text, move |event| emit(&sink, &tag, event));
            tokio::pin!(turn);
            loop {
                tokio::select! {
                    done = &mut turn => break Some(done),
                    line = input.next_line() => {
                        let Ok(Some(line)) = line else {
                            break None;
                        };
                        match rpc::parse(&line) {
                            Ok(Frame::Notification { method, .. }) if method == "session/cancel" => break None,
                            Ok(Frame::Request { id, .. }) => {
                                let busy = Err((-32000, "a prompt is already running".to_string()));
                                drop(out.send(answer(&id, busy)));
                            }
                            Ok(Frame::Notification { .. } | Frame::Response { .. }) | Err(_) => {}
                        }
                    }
                }
            }
        };
        match outcome {
            Some(Ok(turn)) if turn.aborted => Ok(json!({"stopReason": "max_turn_requests"})),
            Some(Ok(_turn)) => Ok(json!({"stopReason": "end_turn"})),
            Some(Err(error)) => {
                drop(agent.abort_note("turn failed"));
                Err(failure(error))
            }
            None => {
                agent.abort_note("cancelled by client").map_err(failure)?;
                Ok(json!({"stopReason": "cancelled"}))
            }
        }
    }
}

fn emit(out: &UnboundedSender<Value>, session: &str, event: &Event) {
    let content = match event {
        Event::ToolCall {
            id,
            name,
            arguments,
        } => json!({
            "sessionUpdate": "tool_call",
            "toolCallId": id,
            "title": name,
            "kind": kind(name),
            "status": "in_progress",
            "rawInput": serde_json::from_str::<Value>(arguments).unwrap_or_else(|_invalid| json!(arguments)),
        }),
        Event::ToolResult {
            id,
            success,
            output,
            ..
        } => json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": id,
            "status": if *success { "completed" } else { "failed" },
            "content": [{"type": "content", "content": {"type": "text", "text": output}}],
        }),
        Event::Note { text } | Event::Aborted { reason: text } => json!({
            "sessionUpdate": "agent_thought_chunk",
            "content": {"type": "text", "text": text},
        }),
        Event::Assistant { .. }
        | Event::Usage { .. }
        | Event::Unreported { .. }
        | Event::Mail { .. }
        | Event::Merged { .. }
        | Event::Delta { .. }
        | Event::Reasoning { .. }
        | Event::Approval { .. }
        | Event::Verify { .. }
        | Event::Receipt { .. }
        | Event::Work { .. }
        | Event::Spawned { .. }
        | Event::Child { .. } => return,
    };
    drop(out.send(update(session, content)));
}

fn update(session: &str, content: Value) -> Value {
    rpc::notification(
        "session/update",
        json!({"sessionId": session, "update": content}),
    )
}

fn answer(id: &Value, reply: Reply) -> Value {
    match reply {
        Ok(result) => rpc::response(id, result),
        Err((code, message)) => rpc::failure(id, code, &message),
    }
}

fn failure(error: Error) -> (i64, String) {
    (-32603, error.to_string())
}

fn kind(name: &str) -> &'static str {
    use crate::tools::Builtin;
    match Builtin::live(name) {
        Some(Builtin::Read) => "read",
        Some(Builtin::Edit | Builtin::Write) => "edit",
        Some(Builtin::RunCommand) => "execute",
        Some(Builtin::Web) => "fetch",
        Some(Builtin::Lsp) => "search",
        Some(Builtin::Skill | Builtin::Todo | Builtin::Task) | None => "other",
    }
}

fn block(item: &Value) -> Option<String> {
    let text = match item.get("type").and_then(Value::as_str)? {
        "text" => item.get("text"),
        "resource" => item.pointer("/resource/text"),
        "resource_link" => item.get("uri"),
        _other => None,
    };
    text.and_then(Value::as_str).map(str::to_string)
}
