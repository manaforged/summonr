use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;

use super::{Reply, Server, Updates, codes, failure, frame_line, missing, session_id};
use crate::endpoint::{self, Endpoint, Requirement};
use crate::hub::Status;
use crate::provider::Client;
use crate::rpc;
use crate::runtime::Loop;
use crate::tools::Event;
use summonr_core::error::Result;

pub(crate) const PREFIX: &str = "_summonr.dev/";
const KEY: &str = "summonr.dev/ext";
const VERSION: u64 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Method {
    SessionList,
    SessionDiff,
    SessionModel,
    Steer,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Note {
    Status,
    Child,
    Usage,
}

const METHODS: [(Method, &str); 4] = [
    (Method::SessionList, "session/list"),
    (Method::SessionDiff, "session/diff"),
    (Method::SessionModel, "session/model"),
    (Method::Steer, "steer"),
];

const NOTES: [(Note, &str); 3] = [
    (Note::Status, "session/status"),
    (Note::Child, "session/child"),
    (Note::Usage, "usage"),
];

pub(super) fn opted(params: &Value) -> bool {
    ["/_meta", "/clientCapabilities/_meta"].iter().any(|at| {
        params
            .pointer(at)
            .and_then(|meta| meta.get(KEY))
            .and_then(|ext| ext.get("version"))
            .and_then(Value::as_u64)
            == Some(VERSION)
    })
}

pub(super) fn capability() -> Value {
    let features: Vec<&str> = METHODS
        .iter()
        .map(|(_, name)| *name)
        .chain(NOTES.iter().map(|(_, name)| *name))
        .collect();
    json!({KEY: {"version": VERSION, "features": features}})
}

pub(super) fn call(opted: bool, method: &str) -> std::result::Result<Method, (i64, String)> {
    let name = method.strip_prefix(PREFIX).filter(|_| opted);
    METHODS
        .iter()
        .find(|(_, known)| Some(*known) == name)
        .map(|(call, _)| *call)
        .ok_or_else(|| rpc::unsupported(method))
}

impl<F> Server<F>
where
    F: FnMut(&Endpoint, Client, &Path, Option<PathBuf>) -> Result<Loop<Client>>,
{
    pub(super) fn extension(&mut self, method: &str, params: &Value) -> Reply {
        match call(self.ext, method)? {
            Method::Steer => self.idle(params),
            Method::SessionList => sessions(params),
            Method::SessionDiff => self.diff(params),
            Method::SessionModel => self.switch(params),
        }
    }

    fn diff(&self, params: &Value) -> Reply {
        let id = session_id(params);
        let seat = self.sessions.get(id).ok_or_else(|| missing(id))?;
        let turn = match params.get("turn") {
            Some(Value::Number(number)) => number.to_string(),
            Some(Value::String(text)) => text.clone(),
            _last => "last".to_string(),
        };
        let turn = turn
            .parse()
            .map_err(|error: summonr_core::error::Error| (codes::INVALID, error.to_string()))?;
        let diff = crate::diff::turn_stats(seat.agent.workspace(), id, turn).map_err(failure)?;
        serde_json::to_value(diff).map_err(|error| (codes::INTERNAL, error.to_string()))
    }

    fn switch(&mut self, params: &Value) -> Reply {
        let id = session_id(params);
        let tag = self
            .sessions
            .get(id)
            .map(|seat| Arc::clone(&seat.tag))
            .ok_or_else(|| missing(id))?;
        let text = |key: &str| params.get(key).and_then(Value::as_str);
        let endpoint = endpoint::resolve(text("endpoint"), text("model"), Requirement::ModelAndKey)
            .map_err(|error| (codes::INVALID, error.to_string()))?;
        let (client, fallbacks) = self.clients(&endpoint, &tag).map_err(failure)?;
        let seat = self.sessions.get_mut(id).ok_or_else(|| missing(id))?;
        seat.agent.set_fallbacks(fallbacks);
        seat.agent.apply_endpoint(&endpoint);
        seat.agent.set_provider(client, endpoint.model.clone());
        seat.endpoint = endpoint.name;
        Ok(json!({}))
    }
}

fn sessions(params: &Value) -> Reply {
    let cwd = params.get("cwd").and_then(Value::as_str);
    let mut found = crate::session::list().map_err(failure)?;
    found.retain(|info| cwd.is_none_or(|cwd| info.cwd.as_deref() == Some(cwd)));
    Ok(json!({"sessions": crate::session::rows(&found)}))
}

pub(super) fn status(out: &UnboundedSender<String>, session: &str, status: Status) {
    let state = match status {
        Status::Idle => "idle",
        Status::Working => "working",
        Status::NeedsYou => "asking",
        Status::Review => "done",
        Status::Failed => "failed",
    };
    note(
        out,
        Note::Status,
        json!({"sessionId": session, "status": state}),
    );
}

pub(super) fn usage(updates: &Updates, used: u64, cost: Option<f64>) {
    note(
        &updates.out,
        Note::Usage,
        json!({
            "sessionId": updates.session,
            "endpoint": updates.endpoint,
            "used": used,
            "limit": updates.window,
            "cost": cost,
        }),
    );
}

pub(super) fn observe(updates: &Updates, event: &Event) {
    if let Some(asking) = crate::hub::asking(event) {
        let state = if asking {
            Status::NeedsYou
        } else {
            Status::Working
        };
        return status(&updates.out, &updates.session, state);
    }
    let Event::Child { id, event } = event else {
        return;
    };
    let Event::Spawned {
        assignment,
        access,
        agent,
        name,
        ..
    } = &**event
    else {
        return;
    };
    let title = [name, agent]
        .into_iter()
        .find(|text| !text.is_empty())
        .map_or_else(
            || assignment.lines().next().unwrap_or_default(),
            String::as_str,
        );
    note(
        &updates.out,
        Note::Child,
        json!({"sessionId": updates.session, "childId": id, "kind": access.name(), "title": title}),
    );
}

fn note(out: &UnboundedSender<String>, kind: Note, params: Value) {
    let Some((_, name)) = NOTES.iter().find(|(known, _)| *known == kind) else {
        return;
    };
    let method = format!("{PREFIX}{name}");
    drop(out.send(frame_line(&rpc::notification(&method, params))));
}
