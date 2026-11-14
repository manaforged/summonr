use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, Stdin};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::endpoint::{self, Endpoint};
use crate::provider::Client;
use crate::rpc::{self, Ask, Frame, Read};
use crate::runtime::{Loop, Steering};
use crate::tools::Event;
use summonr_core::error::{Error, Result};

type Input = rpc::Lines<Stdin>;
type Reply = std::result::Result<Value, (i64, String)>;

mod codes {
    pub(super) const INVALID: i64 = -32602;
    pub(super) const INTERNAL: i64 = -32603;
    pub(super) const NO_SESSION: i64 = -32002;
    pub(super) const BUSY: i64 = -31000;
}

mod config;
mod permission;
mod update;
use update::{chunk, emit, update, usage};

struct Seat {
    agent: Loop<Client>,
    steering: Arc<Steering>,
}

struct Updates {
    out: UnboundedSender<String>,
    session: String,
    window: Option<u32>,
}

struct Server<F> {
    endpoint: std::result::Result<Endpoint, String>,
    build: F,
    out: UnboundedSender<String>,
    sessions: BTreeMap<String, Seat>,
}

pub async fn serve<F>(name: Option<&str>, model: Option<&str>, build: F) -> Result<i32>
where
    F: FnMut(&Endpoint, Client, &Path, Option<PathBuf>) -> Result<Loop<Client>>,
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
    for seat in server.sessions.values() {
        for note in seat.agent.close().await {
            eprintln!("summonr: {note}");
        }
    }
    drop(server);
    drop(writer.await);
    Ok(0)
}

impl<F> Server<F>
where
    F: FnMut(&Endpoint, Client, &Path, Option<PathBuf>) -> Result<Loop<Client>>,
{
    async fn handle(&mut self, line: &str, input: &mut Input) {
        let Ok(Frame::Request { id, method, params }) = rpc::parse(line) else {
            return;
        };
        let reply = match rpc::ask(&method) {
            Ask::Initialize => Ok(json!({
                "protocolVersion": 1,
                "agentCapabilities": {
                    "loadSession": true,
                    "mcpCapabilities": {"http": false, "sse": false}
                },
                "agentInfo": {"name": "summonr", "version": env!("CARGO_PKG_VERSION")},
                "authMethods": []
            })),
            Ask::Authenticate => Ok(json!({})),
            Ask::SessionNew => self.open(&params, "session/new", None).await,
            Ask::SessionLoad => self.load(&params).await,
            Ask::SessionSetConfigOption => self.configure(&params),
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

    async fn load(&mut self, params: &Value) -> Reply {
        let id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if self.sessions.contains_key(id) {
            return Err((codes::INVALID, format!("session {id} is already open")));
        }
        let path = crate::session::resolve_id(id).map_err(|_unknown| missing(id))?;
        self.open(params, "session/load", Some(path)).await
    }

    async fn open(&mut self, params: &Value, method: &str, resume: Option<PathBuf>) -> Reply {
        let cwd = params
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| (codes::INVALID, format!("{method} needs cwd")))?;
        let servers = update::servers(params)?;
        let replay = resume.is_some();
        let endpoint = self
            .endpoint
            .clone()
            .map_err(|error| (codes::INTERNAL, error))?;
        let tag = Arc::new(OnceLock::new());
        let client = self.connect(&endpoint, &tag).map_err(failure)?;
        let mut session = (self.build)(&endpoint, client, &cwd, resume).map_err(failure)?;
        let (fallbacks, skipped) =
            endpoint::fallbacks(&endpoint, |fallback| self.connect(fallback, &tag));
        for note in skipped {
            eprintln!("summonr: {note}");
        }
        session.set_fallbacks(fallbacks);
        session.add_mcp(servers);
        let steering = Arc::new(Steering::approvals_only());
        session.steer_with(Arc::clone(&steering));
        session.begin().await;
        let options = config::options(session.effort());
        let id = session.session_id();
        drop(tag.set(id.clone()));
        if replay {
            for event in crate::hub::journal(session.session_path()) {
                update::replay(&self.out, &id, &event);
            }
        }
        self.sessions.insert(
            id.clone(),
            Seat {
                agent: session,
                steering,
            },
        );
        Ok(json!({"sessionId": id, "configOptions": options}))
    }

    fn configure(&mut self, params: &Value) -> Reply {
        let id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(seat) = self.sessions.get_mut(id) else {
            return Err(missing(id));
        };
        let effort = config::effort(params)?;
        seat.agent.choose_effort(effort);
        if effort.is_none() {
            seat.agent.set_effort(None);
        }
        Ok(json!({"configOptions": config::options(seat.agent.effort())}))
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
            drop(out.send(frame_line(&update(session, chunk(kind, text)))));
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
            return Err(missing(&session));
        };
        let window = agent.context_window();
        let outcome = {
            let updates = Updates {
                out: out.clone(),
                session: session.clone(),
                window,
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
                                let busy = Err((codes::BUSY, "a prompt is already running".to_string()));
                                drop(out.send(frame_line(&rpc::answer(&id, busy))));
                            }
                            Ok(Frame::Response { id, result, .. }) => permission::answer(steering, &id, result.as_ref()),
                            Ok(Frame::Notification { .. }) | Err(_) => {}
                        }
                    }
                }
            }
        };
        match outcome {
            Some(Ok(turn)) => config::stop(&turn),
            Some(Err(error)) => Err(failure(error)),
            None => {
                agent.abort_note("cancelled by client").map_err(failure)?;
                Ok(json!({"stopReason": "cancelled"}))
            }
        }
    }
}

impl Updates {
    fn emit(&self, event: &Event) {
        match event {
            Event::Approval {
                detail: Some(detail),
                id,
                persist,
                dialog,
            } => return self.ask(detail, id.as_deref(), *persist, *dialog),
            Event::Usage {
                prompt_tokens,
                completion_tokens,
                ..
            } => usage(
                &self.out,
                &self.session,
                self.window,
                u64::from(*prompt_tokens) + u64::from(*completion_tokens),
            ),
            _other => {}
        }
        emit(&self.out, &self.session, event);
    }

    fn ask(&self, detail: &str, call: Option<&str>, persist: bool, dialog: u64) {
        let request = permission::request(&self.session, detail, call, persist, dialog);
        drop(self.out.send(frame_line(&request)));
    }
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
    (codes::INTERNAL, error.to_string())
}

fn missing(session: &str) -> (i64, String) {
    (codes::NO_SESSION, format!("unknown session {session}"))
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
