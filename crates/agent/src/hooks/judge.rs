use serde::Deserialize;
use serde_json::Value;

use super::{Dialect, HookEvent};

pub(super) struct Outcome {
    pub(super) code: Option<i32>,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

#[derive(Default)]
pub(crate) struct Verdict {
    pub(crate) blocked: Option<String>,
    pub(crate) context: Vec<String>,
    pub(crate) notes: Vec<String>,
    pub(crate) halted: Option<String>,
    pub(crate) ask: Option<String>,
    pub(crate) input: Option<Value>,
    pub(crate) allowed: bool,
}

impl Verdict {
    pub(crate) fn merge(&mut self, other: Self) {
        self.blocked = match (self.blocked.take(), other.blocked) {
            (Some(first), Some(second)) => Some(format!("{first}; {second}")),
            (first, second) => first.or(second),
        };
        self.context.extend(other.context);
        self.notes.extend(other.notes);
        self.halted = self.halted.take().or(other.halted);
        self.ask = self.ask.take().or(other.ask);
        self.input = other.input.or_else(|| self.input.take());
        self.allowed |= other.allowed;
    }

    pub(crate) fn feedback(&self) -> Vec<&str> {
        self.blocked
            .iter()
            .chain(&self.context)
            .map(String::as_str)
            .collect()
    }

    pub(crate) fn note(&self) -> Option<String> {
        let mut parts = self.feedback();
        parts.extend(self.notes.iter().chain(&self.halted).map(String::as_str));
        (!parts.is_empty()).then(|| parts.join("; "))
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Decision {
    Allow,
    Approve,
    Ask,
    Deny,
    Block,
    Defer,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Specific {
    permission_decision: Option<Decision>,
    permission_decision_reason: Option<String>,
    updated_input: Option<Value>,
    additional_context: Option<String>,
    decision: Option<Request>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    behavior: Option<Decision>,
    updated_input: Option<Value>,
    message: Option<String>,
}

impl Request {
    fn apply(self, verdict: &mut Verdict) {
        match self.behavior {
            Some(Decision::Allow | Decision::Approve) => {
                verdict.allowed = true;
                verdict.input = self.updated_input.or_else(|| verdict.input.take());
            }
            Some(Decision::Deny | Decision::Block) => {
                verdict.blocked = Some(self.message.unwrap_or_else(|| {
                    format!("denied by a {} hook", HookEvent::PermissionRequest.claude())
                }));
            }
            Some(Decision::Ask | Decision::Defer) | None => {}
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    #[serde(rename = "continue")]
    proceed: Option<bool>,
    stop_reason: Option<String>,
    system_message: Option<String>,
    decision: Option<Decision>,
    reason: Option<String>,
    hook_specific_output: Option<Specific>,
}

impl Output {
    fn apply(self, event: HookEvent, verdict: &mut Verdict) {
        let specific = self.hook_specific_output.unwrap_or_default();
        if keeps_control(event) {
            if self.proceed == Some(false) {
                verdict.halted =
                    Some(self.stop_reason.unwrap_or_else(|| {
                        format!("a {} hook stopped the session", event.claude())
                    }));
            }
            verdict.notes.extend(self.system_message);
        }
        let reason = specific.permission_decision_reason.or(self.reason);
        match specific.permission_decision.or(self.decision) {
            Some(Decision::Deny | Decision::Block) => {
                verdict.blocked =
                    Some(reason.unwrap_or_else(|| format!("blocked by a {} hook", event.claude())));
            }
            Some(Decision::Ask) => verdict.ask = Some(reason.unwrap_or_default()),
            Some(Decision::Defer) => verdict
                .notes
                .push("summonr does not defer tool calls; the call goes on to policy".to_string()),
            Some(Decision::Allow | Decision::Approve) | None => {}
        }
        if verdict.blocked.is_none() {
            verdict.input = specific.updated_input;
        }
        verdict.context.extend(specific.additional_context);
        if let Some(request) = specific
            .decision
            .filter(|_| event == HookEvent::PermissionRequest)
        {
            request.apply(verdict);
        }
    }
}

enum Stdout {
    Json(Box<Output>),
    Invalid(String),
    Plain(String),
}

impl Stdout {
    fn read(text: &str) -> Self {
        let text = text.trim();
        if !(text.starts_with('{') && text.ends_with('}')) {
            return Self::Plain(text.to_string());
        }
        serde_json::from_str(text).map_or_else(
            |error| Self::Invalid(error.to_string()),
            |output| Self::Json(Box::new(output)),
        )
    }
}

const fn blocks(event: HookEvent) -> bool {
    matches!(
        event,
        HookEvent::PreTool
            | HookEvent::Prompt
            | HookEvent::Stop
            | HookEvent::SubagentStop
            | HookEvent::PreCompact
    )
}

const fn gates(event: HookEvent) -> bool {
    blocks(event) && !matches!(event, HookEvent::Stop | HookEvent::SubagentStop)
}

const fn ignores_exit(event: HookEvent) -> bool {
    matches!(
        event,
        HookEvent::PermissionRequest | HookEvent::Notification | HookEvent::StopFailure
    )
}

const fn reads_stdout(event: HookEvent) -> bool {
    matches!(
        event,
        HookEvent::SessionStart | HookEvent::Prompt | HookEvent::PreCompact
    )
}

const fn keeps_control(event: HookEvent) -> bool {
    !matches!(
        event,
        HookEvent::SessionEnd
            | HookEvent::PreCompact
            | HookEvent::Compact
            | HookEvent::Notification
            | HookEvent::StopFailure
    )
}

pub(super) fn judge(dialect: Dialect, event: HookEvent, outcome: &Outcome) -> Verdict {
    let mut verdict = Verdict::default();
    let stdout = Stdout::read(&outcome.stdout);
    let json = matches!(stdout, Stdout::Json(_));
    match stdout {
        Stdout::Json(output) => output.apply(event, &mut verdict),
        Stdout::Plain(text)
            if outcome.code == Some(0) && reads_stdout(event) && !text.is_empty() =>
        {
            verdict.context.push(text);
        }
        Stdout::Invalid(error) if dialect == Dialect::Claude && outcome.code != Some(2) => {
            verdict.notes.push(format!("invalid JSON output: {error}"));
        }
        Stdout::Plain(_) | Stdout::Invalid(_) => {}
    }
    let reason = if outcome.stderr.is_empty() {
        format!("exited with {:?}", outcome.code)
    } else {
        outcome.stderr.clone()
    };
    match outcome.code {
        Some(0) => {}
        Some(2) if ignores_exit(event) => {}
        Some(2) if blocks(event) => verdict.blocked = verdict.blocked.take().or(Some(reason)),
        Some(2) if matches!(event, HookEvent::PostTool | HookEvent::ToolFailure) => {
            verdict.context.push(reason);
        }
        Some(2) => verdict.notes.push(reason),
        Some(_) | None if dialect == Dialect::Summonr && gates(event) => {
            verdict.blocked = verdict.blocked.take().or(Some(reason));
        }
        Some(_) | None if dialect == Dialect::Claude && json => {}
        Some(_) | None => verdict.notes.push(reason),
    }
    verdict
}

pub(super) fn failed(dialect: Dialect, event: HookEvent, error: String) -> Verdict {
    let mut verdict = Verdict::default();
    if dialect == Dialect::Summonr && gates(event) {
        verdict.blocked = Some(error);
    } else {
        verdict.notes.push(error);
    }
    verdict
}
