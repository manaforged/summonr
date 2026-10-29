use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::process::{ChildStdin, ChildStdout};

use crate::process::{Cap, Child, Tail};
use crate::rpc::{Framing, Lines, Read};

use super::super::reasoning::{EffortField, Off, Plan, Step};
use super::super::{CompletionRequest, Effort, Message, Role};
use super::frame::{Frame, Ready, ReplyKind};
use super::{AgentClient, bridge};
use summonr_core::error::{Error, ProviderFailureKind, Result};

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

const EFFORT: Plan<'static> = Plan {
    field: EffortField::ReasoningEffort,
    levels: &[
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::XHigh,
        Effort::Max,
    ],
    off: Off::Unsupported,
    up: false,
};

struct Spec<'a> {
    model: &'a str,
    system: Option<&'a str>,
    effort: Option<Effort>,
    thread: Thread,
}

pub(in crate::provider) struct Run {
    child: Child,
    _group: Option<crate::tools::command::ProcessGroup>,
    stdin: ChildStdin,
    pub(super) reader: Lines<ChildStdout>,
    stderr: Tail,
    secrets: Vec<String>,
    pub(super) listing: Vec<Value>,
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
    shape: String,
}

impl Drop for Run {
    fn drop(&mut self) {
        let _killed = self.child.start_kill();
    }
}

impl Run {
    pub(super) async fn start(
        agent: &AgentClient,
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
            thread,
        };
        let listing = super::super::wire::tools(request, "inputSchema", &[]);
        let mut run = Self::open(agent, spec, listing).await?;
        run.shape = request.shape_hash();
        for message in input {
            run.send(&bridge::user(message)).await?;
        }
        Ok(run)
    }

    async fn open(agent: &AgentClient, spec: Spec<'_>, listing: Vec<Value>) -> Result<Self> {
        let prompt = match spec.system {
            Some(system) => {
                let mut file = tempfile::NamedTempFile::new()?;
                std::io::Write::write_all(&mut file, system.as_bytes())?;
                Some(file)
            }
            None => None,
        };
        let args = arguments(&spec, prompt.as_ref().map(tempfile::NamedTempFile::path));
        let cwd = std::env::current_dir()?;
        let process = crate::process::start(
            crate::process::Spec {
                remove: agent.unset.iter().map(Into::into).collect(),
                cwd: Some(cwd),
                ..crate::process::Spec::new(&agent.program, args)
            }
            .envs(agent.env.iter().cloned()),
        )
        .map_err(|error| {
            Error::Config(format!(
                "{error}; install Claude Code and run `claude auth login`"
            ))
        })?;
        let stderr = Tail::spawn(
            process.stderr,
            Cap::Tail(crate::limits::limits().mcp_stderr_tail_bytes),
        );
        let thread = match &spec.thread {
            Thread::Start(id) | Thread::Resume(id) => Some(id.clone()),
            Thread::Once => None,
        };
        let mut run = Self {
            child: process.child,
            _group: process.group,
            stdin: process.stdin,
            reader: Lines::framed(
                process.stdout,
                Framing::Line,
                crate::limits::limits().agent_line_max_bytes,
            ),
            stderr,
            secrets: agent.env.iter().map(|(_, value)| value.clone()).collect(),
            listing,
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
            shape: String::new(),
        };
        run.send(&json!({
            "type": "control_request",
            "request_id": INIT,
            "request": {"subtype": "initialize", "hooks": null}
        }))
        .await?;
        run.handshake().await?;
        keep_listing(&agent.name, &run.ready);
        Ok(run)
    }

    async fn handshake(&mut self) -> Result<()> {
        loop {
            let line = self.line().await?;
            let Some(frame) = Self::frame(&line)? else {
                continue;
            };
            match frame {
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

    pub(super) fn fits(&mut self, request: &CompletionRequest<'_>) -> bool {
        self.shape == request.shape_hash() && matches!(self.child.try_wait(), Ok(None))
    }

    pub(super) fn recorded(&self) -> Option<&str> {
        self.thread.as_deref().filter(|_| self.seen)
    }

    pub(super) async fn line(&mut self) -> Result<String> {
        match self.reader.next().await? {
            Read::Line(line) => Ok(line),
            Read::Oversize => Err(Error::provider(
                ProviderFailureKind::Malformed,
                format!(
                    "Claude Code sent a line over {} MiB; summonr dropped it",
                    crate::limits::limits().agent_line_max_bytes / (1024 * 1024)
                ),
            )),
            Read::Closed => Err(self.exited("")),
        }
    }

    pub(super) fn frame(line: &str) -> Result<Option<Frame<'_>>> {
        match serde_json::from_str::<Frame<'_>>(line) {
            Ok(frame) => Ok(Some(frame)),
            Err(error) => match unanswerable(line) {
                Some(subtype) => Err(Error::provider(
                    ProviderFailureKind::Malformed,
                    format!(
                        "Claude Code sent a {subtype} control request summonr could not read: {error}"
                    ),
                )),
                None => Ok(None),
            },
        }
    }

    pub(super) fn exited(&self, cause: &str) -> Error {
        let note = self
            .stderr
            .last_line(&self.secrets)
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

fn keep_listing(name: &str, ready: &Value) {
    if crate::catalog::listing(name).is_some_and(|listing| !listing.stale()) {
        return;
    }
    let ready: Ready = serde_json::from_value(ready.clone()).unwrap_or_default();
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
        return;
    }
    crate::catalog::enrich(name, None, &mut models);
    drop(crate::catalog::remember(name, models));
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
    if let Step::Level(effort) = EFFORT.step(spec.effort) {
        args.extend(["--effort".to_string(), effort.as_str().to_string()]);
    }
    match &spec.thread {
        Thread::Start(id) => args.extend(["--session-id".to_string(), id.clone()]),
        Thread::Resume(id) => args.extend(["--resume".to_string(), id.clone()]),
        Thread::Once => args.push("--no-session-persistence".to_string()),
    }
    args
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
