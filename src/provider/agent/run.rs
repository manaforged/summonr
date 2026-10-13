use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};

use super::super::{CompletionRequest, Effort, Message, Role, ToolDefinition};
use super::frame::{Frame, Ready, ReplyKind};
use super::{Seat, bridge};
use crate::error::{Error, ProviderFailureKind, Result};

const INIT: &str = "init";

pub(in crate::provider) enum Thread {
    Start(String),
    Resume(String),
    Once,
}

pub(super) struct Call {
    pub(super) request: String,
    pub(super) rpc: Value,
    pub(super) id: String,
    pub(super) name: String,
    pub(super) arguments: Value,
}

struct Spec<'a> {
    model: &'a str,
    system: Option<&'a str>,
    effort: Option<Effort>,
    tools: &'a [ToolDefinition],
    thread: Thread,
}

pub(in crate::provider) struct Run {
    child: Child,
    _group: Option<crate::tools::command::ProcessGroup>,
    stdin: ChildStdin,
    pub(super) reader: BufReader<ChildStdout>,
    stderr: Arc<Mutex<Vec<u8>>>,
    drain: Option<tokio::task::JoinHandle<()>>,
    pub(super) tools: Vec<ToolDefinition>,
    pub(super) pending: Vec<Call>,
    thread: Option<String>,
    pub(super) seen: bool,
    pub(super) rejected: bool,
    pub(super) text: String,
    pub(super) reasoning: String,
    pub(super) sizes: (usize, usize),
    _prompt: Option<tempfile::NamedTempFile>,
    pub(super) calls: Vec<(String, super::super::Usage)>,
    pub(super) metered: bool,
    pub(super) spent: (u32, u32),
    ready: Value,
}

impl Drop for Run {
    fn drop(&mut self) {
        if let Some(drain) = &self.drain {
            drain.abort();
        }
        let _killed = self.child.start_kill();
    }
}

impl Run {
    pub(super) async fn start(
        seat: &Seat,
        request: &CompletionRequest<'_>,
        thread: Thread,
        input: &[Message],
    ) -> Result<Self> {
        let system = request
            .messages
            .first()
            .filter(|message| message.role == Role::System)
            .map(|message| message.content.as_str());
        let spec = Spec {
            model: &request.model,
            system,
            effort: request.effort,
            tools: request.tools,
            thread,
        };
        let mut run = Self::open(seat, spec).await?;
        for message in input {
            run.send(&bridge::user(message)).await?;
        }
        Ok(run)
    }

    async fn open(seat: &Seat, spec: Spec<'_>) -> Result<Self> {
        let prompt = match spec.system {
            Some(system) => {
                let mut file = tempfile::NamedTempFile::new()?;
                std::io::Write::write_all(&mut file, system.as_bytes())?;
                Some(file)
            }
            None => None,
        };
        let args = arguments(&spec, prompt.as_ref().map(tempfile::NamedTempFile::path));
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let cwd = std::env::current_dir()?;
        let process = crate::rpc::start(&crate::rpc::Launch {
            program: &seat.program,
            args: &args,
            env: &seat.env,
            unset: seat.unset,
            cwd: &cwd,
            sandbox: None,
            base: crate::rpc::Env::Inherit,
            stderr: crate::rpc::Stderr::Pipe,
        })
        .map_err(|error| {
            Error::Config(format!(
                "{error}; install Claude Code and run `claude auth login`"
            ))
        })?;
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let drain = process
            .stderr
            .map(|pipe| tokio::spawn(crate::rpc::tail(pipe, Arc::clone(&stderr))));
        let thread = match &spec.thread {
            Thread::Start(id) | Thread::Resume(id) => Some(id.clone()),
            Thread::Once => None,
        };
        let mut run = Self {
            child: process.child,
            _group: process.group,
            stdin: process.stdin,
            reader: BufReader::new(process.stdout),
            stderr,
            drain,
            tools: spec.tools.to_vec(),
            pending: Vec::new(),
            thread,
            seen: false,
            rejected: false,
            text: String::new(),
            reasoning: String::new(),
            sizes: (0, 0),
            _prompt: prompt,
            calls: Vec::new(),
            metered: false,
            spent: (0, 0),
            ready: Value::Null,
        };
        run.send(&json!({
            "type": "control_request",
            "request_id": INIT,
            "request": {"subtype": "initialize", "hooks": null}
        }))
        .await?;
        run.handshake().await?;
        Ok(run)
    }

    async fn handshake(&mut self) -> Result<()> {
        loop {
            match self.event().await? {
                Frame::ControlResponse { response } if response.request_id == INIT => {
                    if response.subtype == ReplyKind::Error {
                        return Err(Error::provider(
                            ProviderFailureKind::Authentication,
                            format!("Claude Code refused to start: {}", response.error),
                        ));
                    }
                    self.ready = response.response;
                    return Ok(());
                }
                Frame::ControlRequest {
                    request_id,
                    request,
                } => {
                    if let Some(call) = self.control(request_id, request).await? {
                        self.pending.push(call);
                    }
                }
                Frame::ControlResponse { .. }
                | Frame::StreamEvent { .. }
                | Frame::Assistant { .. }
                | Frame::RateLimitEvent { .. }
                | Frame::Result(..)
                | Frame::Other => {}
            }
        }
    }

    pub(super) fn recorded(&self) -> Option<&str> {
        self.thread.as_deref().filter(|_| self.seen)
    }

    pub(super) async fn event(&mut self) -> Result<Frame> {
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line).await? == 0 {
                return Err(self.exited(""));
            }
            match serde_json::from_str::<Frame>(&line) {
                Ok(frame) => return Ok(frame),
                Err(error) => {
                    if let Some(subtype) = unanswerable(&line) {
                        return Err(Error::provider(
                            ProviderFailureKind::Malformed,
                            format!(
                                "Claude Code sent a {subtype} control request summonr could not read: {error}"
                            ),
                        ));
                    }
                }
            }
        }
    }

    pub(super) fn exited(&self, cause: &str) -> Error {
        let tail = self
            .stderr
            .lock()
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
            .unwrap_or_default();
        let note = tail
            .lines()
            .last()
            .map(|last| format!(": {last}"))
            .unwrap_or_default();
        Error::provider(
            ProviderFailureKind::Http,
            format!("Claude Code exited before the turn finished{cause}{note}"),
        )
    }

    pub(super) async fn send(&mut self, value: &Value) -> Result<()> {
        let mut line = serde_json::to_string(value)?;
        line.push('\n');
        let written = self.stdin.write_all(line.as_bytes()).await;
        let flushed = self.stdin.flush().await;
        written
            .and(flushed)
            .map_err(|error| self.exited(&format!(" ({error})")))
    }
}

pub(super) async fn models(seat: &Seat) -> Result<Vec<crate::catalog::Model>> {
    let run = Run::open(
        seat,
        Spec {
            model: "",
            system: None,
            effort: None,
            tools: &[],
            thread: Thread::Once,
        },
    )
    .await?;
    let ready: Ready = serde_json::from_value(run.ready.clone()).unwrap_or_default();
    let mut models: Vec<crate::catalog::Model> = ready
        .models
        .into_iter()
        .map(|row| crate::catalog::Model {
            id: row.resolved_model,
            display: row.display_name,
            about: row.description,
            cost: crate::catalog::Cost::Plan,
            reasoning: row.supports_effort,
            ..crate::catalog::Model::default()
        })
        .collect();
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    if models.is_empty() {
        return Err(Error::provider(
            ProviderFailureKind::Malformed,
            "Claude Code listed no models",
        ));
    }
    Ok(models)
}

fn unanswerable(line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;
    (value["type"] == "control_request").then(|| {
        value["request"]["subtype"]
            .as_str()
            .unwrap_or("unnamed")
            .to_string()
    })
}

fn arguments(spec: &Spec<'_>, prompt: Option<&std::path::Path>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--input-format",
        "stream-json",
        "--include-partial-messages",
        "--tools",
        "",
        "--setting-sources=",
        "--strict-mcp-config",
        "--permission-prompt-tool",
        "stdio",
        "--mcp-config",
    ]
    .map(str::to_string)
    .into();
    args.push(
        json!({"mcpServers": {bridge::SERVER: {"type": "sdk", "name": bridge::SERVER}}})
            .to_string(),
    );
    if !spec.model.is_empty() {
        args.extend(["--model".to_string(), spec.model.to_string()]);
    }
    if let Some(prompt) = prompt {
        args.extend([
            "--system-prompt-file".to_string(),
            prompt.display().to_string(),
            "--system-prompt-snapshot".to_string(),
            "off".to_string(),
        ]);
    }
    if let Some(effort) = spec.effort {
        args.extend(["--effort".to_string(), level(effort).to_string()]);
    }
    match &spec.thread {
        Thread::Start(id) => args.extend(["--session-id".to_string(), id.clone()]),
        Thread::Resume(id) => args.extend(["--resume".to_string(), id.clone()]),
        Thread::Once => args.push("--no-session-persistence".to_string()),
    }
    args
}

fn level(effort: Effort) -> &'static str {
    match effort {
        Effort::None | Effort::Minimal | Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::XHigh => "xhigh",
        Effort::Max => "max",
    }
}
