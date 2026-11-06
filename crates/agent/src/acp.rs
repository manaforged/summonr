use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, Stdin};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::UserInput;
use crate::endpoint::{self, Endpoint};
use crate::provider::Client;
use crate::rpc::{self, Ask, Frame, Read};
use crate::runtime::{GrantAnswer, Loop, Steering};
use crate::tools::Event;
use summonr_core::error::{Error, Result};

type Input = rpc::Lines<Stdin>;
type Reply = std::result::Result<Value, (i64, String)>;

const PERMISSION: &str = "permission-";

mod update;
use update::{emit, update};

struct Choice {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    answer: GrantAnswer,
}

const CHOICES: [Choice; 3] = [
    Choice {
        id: "allow-once",
        name: "Allow once",
        kind: "allow_once",
        answer: GrantAnswer::Allow,
    },
    Choice {
        id: "allow-always",
        name: "Always allow",
        kind: "allow_always",
        answer: GrantAnswer::Always,
    },
    Choice {
        id: "reject-once",
        name: "Reject",
        kind: "reject_once",
        answer: GrantAnswer::Deny,
    },
];

struct Seat {
    agent: Loop<Client>,
    steering: Arc<Steering>,
}

struct Updates {
    out: UnboundedSender<String>,
    session: String,
    open: Vec<String>,
}

struct Server<F> {
    endpoint: std::result::Result<Endpoint, String>,
    build: F,
    out: UnboundedSender<String>,
    sessions: BTreeMap<String, Seat>,
}

pub async fn serve<F>(name: Option<&str>, model: Option<&str>, build: F) -> Result<i32>
where
    F: FnMut(&Endpoint, Client, &Path) -> Result<Loop<Client>>,
{
    let (out, mut queue) = unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(line) = queue.recv().await {
            if stdout.write_all(line.as_bytes()).await.is_err() || stdout.flush().await.is_err() {
                break;
            }
        }
    });
    let mut server = Server {
        endpoint: endpoint::resolve(name, model, crate::endpoint::Requirement::ModelAndKey)
            .map_err(|error| error.to_string()),
        build,
        out,
        sessions: BTreeMap::new(),
    };
    let mut input = rpc::Lines::new(tokio::io::stdin());
    while let Some(line) = next_line(&mut input).await {
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
        let reply = match rpc::ask(&method) {
            Ask::Initialize => Ok(json!({
                "protocolVersion": 1,
                "agentCapabilities": {"loadSession": false},
                "authMethods": []
            })),
            Ask::Authenticate => Ok(json!({})),
            Ask::SessionNew => self.open(&params),
            Ask::SessionPrompt => self.prompt(&params, input).await,
            Ask::Roots
            | Ask::Elicit
            | Ask::Ping
            | Ask::SessionCancel
            | Ask::ToolsList
            | Ask::ToolsCall
            | Ask::Unsupported => Err(rpc::unsupported(&method)),
        };
        drop(self.out.send(frame_line(&rpc::answer(&id, reply))));
    }

    fn connect(&self, endpoint: &Endpoint, tag: &Arc<OnceLock<String>>) -> Result<Client> {
        let message = self.stream("agent_message_chunk", tag);
        let thought = self.stream("agent_thought_chunk", tag);
        Ok(
            Client::connect(endpoint)?.on_stream(move |delta| match delta {
                crate::Delta::Text(text) => message(text),
                crate::Delta::Thinking(text) => thought(text),
                crate::Delta::Call(_) | crate::Delta::Progress => {}
            }),
        )
    }

    fn open(&mut self, params: &Value) -> Reply {
        let cwd = params
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| (-32602, "session/new needs cwd".to_string()))?;
        let endpoint = self.endpoint.clone().map_err(|error| (-32603, error))?;
        let tag = Arc::new(OnceLock::new());
        let client = self.connect(&endpoint, &tag).map_err(failure)?;
        let mut session = (self.build)(&endpoint, client, &cwd).map_err(failure)?;
        let (fallbacks, skipped) =
            endpoint::fallbacks(&endpoint, |fallback| self.connect(fallback, &tag));
        for note in skipped {
            eprintln!("summonr: {note}");
        }
        session.set_fallbacks(fallbacks);
        let steering = Arc::new(Steering::approvals_only());
        session.steer_with(Arc::clone(&steering));
        let id = session.session_id();
        drop(tag.set(id.clone()));
        self.sessions.insert(
            id.clone(),
            Seat {
                agent: session,
                steering,
            },
        );
        Ok(json!({"sessionId": id}))
    }

    fn stream(
        &self,
        kind: &'static str,
        tag: &Arc<OnceLock<String>>,
    ) -> impl Fn(&str) + Send + Sync + 'static {
        let out = self.out.clone();
        let tag = tag.clone();
        move |text: &str| {
            let session = tag.get().map_or("", String::as_str);
            let content = json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}});
            drop(out.send(frame_line(&update(session, content))));
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
        let out = self.out.clone();
        let Some(Seat { agent, steering }) = self.sessions.get_mut(&session) else {
            return Err((-32602, format!("unknown session {session}")));
        };
        let outcome = {
            let mut updates = Updates {
                out: out.clone(),
                session: session.clone(),
                open: Vec::new(),
            };
            let turn = agent.turn_with(text, move |event| updates.emit(event));
            tokio::pin!(turn);
            loop {
                tokio::select! {
                    done = &mut turn => break Some(done),
                    read = next_line(input) => {
                        let Some(line) = read else {
                            break None;
                        };
                        match rpc::parse(&line) {
                            Ok(Frame::Notification { method, params })
                                if rpc::ask(&method) == Ask::SessionCancel
                                    && params.get("sessionId").and_then(Value::as_str) == Some(session.as_str()) =>
                            {
                                break None;
                            }
                            Ok(Frame::Request { id, .. }) => {
                                let busy = Err((-32000, "a prompt is already running".to_string()));
                                drop(out.send(frame_line(&rpc::answer(&id, busy))));
                            }
                            Ok(Frame::Response { id, result, .. }) => answer(steering, &id, result.as_ref()),
                            Ok(Frame::Notification { .. }) | Err(_) => {}
                        }
                    }
                }
            }
        };
        match outcome {
            Some(Ok(turn)) if turn.aborted => Ok(json!({"stopReason": "max_turn_requests"})),
            Some(Ok(_turn)) => Ok(json!({"stopReason": "end_turn"})),
            Some(Err(error)) => Err(failure(error)),
            None => {
                agent.abort_note("cancelled by client").map_err(failure)?;
                Ok(json!({"stopReason": "cancelled"}))
            }
        }
    }
}

impl Updates {
    fn emit(&mut self, event: &Event) {
        match event {
            Event::ToolCall { id, .. } => self.open.push(id.clone()),
            Event::ToolResult { id, .. } => self.open.retain(|open| open != id),
            Event::Approval {
                detail: Some(detail),
                persist,
                dialog,
            } => return self.ask(detail, *persist, *dialog),
            _other => {}
        }
        emit(&self.out, &self.session, event);
    }

    fn ask(&self, detail: &str, persist: bool, dialog: u64) {
        let options = CHOICES
            .iter()
            .filter(|choice| persist || choice.answer != GrantAnswer::Always)
            .map(|choice| json!({"optionId": choice.id, "name": choice.name, "kind": choice.kind}))
            .collect::<Vec<_>>();
        let call = self.open.last().map_or("", String::as_str);
        let params = json!({
            "sessionId": self.session,
            "toolCall": {"toolCallId": call, "title": detail},
            "options": options,
        });
        let id = json!(format!("{PERMISSION}{dialog}"));
        let request = rpc::request(&id, "session/request_permission", params);
        drop(self.out.send(frame_line(&request)));
    }
}

fn answer(steering: &Steering, id: &Value, result: Option<&Value>) {
    let Some(dialog) = id
        .as_str()
        .and_then(|id| id.strip_prefix(PERMISSION))
        .and_then(|dialog| dialog.parse::<u64>().ok())
    else {
        return;
    };
    let outcome = result.and_then(|result| result.get("outcome"));
    let picked = outcome
        .filter(|outcome| outcome.get("outcome").and_then(Value::as_str) == Some("selected"))
        .and_then(|outcome| outcome.get("optionId"))
        .and_then(Value::as_str);
    let decision = CHOICES
        .iter()
        .find(|choice| Some(choice.id) == picked)
        .map_or(GrantAnswer::Deny, |choice| choice.answer);
    steering.answer(dialog, UserInput::from(decision.token()));
}

async fn next_line(input: &mut Input) -> Option<String> {
    loop {
        match input.next().await {
            Ok(Read::Line(line)) => return Some(line),
            Ok(Read::Oversize) => {}
            Ok(Read::Closed) | Err(_) => return None,
        }
    }
}

fn frame_line(frame: &Value) -> String {
    format!("{frame}\n")
}

fn failure(error: Error) -> (i64, String) {
    (-32603, error.to_string())
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
