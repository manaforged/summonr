use serde_json::{Value, json};

use summonr_core::error::{Error, Result};

mod client;
mod frame;
mod peer;

pub(crate) use client::{Client, HttpTransport, Pending, Spawn};
pub(crate) use frame::{Framing, Lines, Read};
pub(crate) use peer::{Lsp, Peer};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Frame {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Response {
        id: Value,
        result: Option<Value>,
        error: Option<Value>,
    },
}

pub(crate) fn parse(line: &str) -> Result<Frame> {
    frame(serde_json::from_str(line)?)
}

pub(crate) fn frame(mut value: Value) -> Result<Frame> {
    let mut take = |key: &str| value.get_mut(key).map(Value::take);
    let method = take("method");
    let id = take("id");
    let params = take("params").unwrap_or(Value::Null);
    match (method.as_ref().and_then(Value::as_str), id) {
        (Some(method), Some(id)) if !id.is_null() => Ok(Frame::Request {
            id,
            method: method.to_string(),
            params,
        }),
        (Some(method), _) => Ok(Frame::Notification {
            method: method.to_string(),
            params,
        }),
        (None, Some(id)) => Ok(Frame::Response {
            id,
            result: take("result"),
            error: take("error"),
        }),
        (None, None) => Err(Error::Tool("frame has neither method nor id".to_string())),
    }
}

pub(crate) fn oversize(method: &str, max: usize) -> Error {
    Error::Tool(format!(
        "a reply to `{method}` was over {} MiB; summonr dropped it",
        max / (1024 * 1024)
    ))
}

pub(crate) fn outcome(method: &str, result: Option<Value>, error: Option<Value>) -> Result<Value> {
    if let Some(error) = error {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(Error::Tool(format!("`{method}` failed: {message}")));
    }
    Ok(result.unwrap_or(Value::Null))
}

pub(crate) enum Reply {
    Result(Value),
    Error(i64, String),
}

pub(crate) enum Notice<'a> {
    ToolsChanged,
    PromptsChanged,
    ResourcesChanged,
    Elicit {
        params: &'a Value,
        answer: tokio::sync::oneshot::Sender<Reply>,
    },
    Progress {
        token: &'a Value,
        progress: f64,
        total: Option<f64>,
        message: Option<&'a str>,
    },
    Message {
        level: &'a str,
        logger: Option<&'a str>,
        data: &'a Value,
    },
    Unknown,
}

pub(crate) fn notice<'a>(method: &str, params: &'a Value) -> Notice<'a> {
    match method {
        "notifications/tools/list_changed" => Notice::ToolsChanged,
        "notifications/prompts/list_changed" => Notice::PromptsChanged,
        "notifications/resources/list_changed" => Notice::ResourcesChanged,
        "notifications/progress" => {
            match (
                params.get("progressToken"),
                params.get("progress").and_then(Value::as_f64),
            ) {
                (Some(token), Some(progress)) => Notice::Progress {
                    token,
                    progress,
                    total: params.get("total").and_then(Value::as_f64),
                    message: params.get("message").and_then(Value::as_str),
                },
                (Some(_) | None, Some(_) | None) => Notice::Unknown,
            }
        }
        "notifications/message" => Notice::Message {
            level: params
                .get("level")
                .and_then(Value::as_str)
                .unwrap_or("info"),
            logger: params.get("logger").and_then(Value::as_str),
            data: params.get("data").unwrap_or(&Value::Null),
        },
        _unknown => Notice::Unknown,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    Roots,
    Elicit,
    Ping,
    Initialize,
    Authenticate,
    SessionNew,
    SessionPrompt,
    SessionCancel,
    ToolsList,
    ToolsCall,
    Unsupported,
}

pub(crate) fn ask(method: &str) -> Ask {
    match method {
        "roots/list" => Ask::Roots,
        "elicitation/create" => Ask::Elicit,
        "ping" => Ask::Ping,
        "initialize" => Ask::Initialize,
        "authenticate" => Ask::Authenticate,
        "session/new" => Ask::SessionNew,
        "session/prompt" => Ask::SessionPrompt,
        "session/cancel" => Ask::SessionCancel,
        "tools/list" => Ask::ToolsList,
        "tools/call" => Ask::ToolsCall,
        _unsupported => Ask::Unsupported,
    }
}

pub(crate) const NOT_FOUND: i64 = -32601;

pub(crate) fn unsupported(method: &str) -> (i64, String) {
    (NOT_FOUND, format!("{method} is not supported"))
}

pub(crate) fn answer(id: &Value, reply: std::result::Result<Value, (i64, String)>) -> Value {
    match reply {
        Ok(result) => response(id, result),
        Err((code, message)) => failure(id, code, &message),
    }
}

pub(crate) fn request(id: &Value, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

pub(crate) fn response(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub(crate) fn failure(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

pub(crate) fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}
