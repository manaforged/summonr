use serde_json::{Value, json};

use crate::error::{Error, Result};

mod client;

pub(crate) use client::{Client, Env, Launch, Process, Spawn, Stderr, start};

pub(crate) const PROTOCOL: &str = "2025-11-25";
const SUPPORTED: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

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
    let mut value: Value = serde_json::from_str(line)?;
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

pub(crate) fn hello(name: &str, capabilities: Value) -> Value {
    json!({
        "protocolVersion": PROTOCOL,
        "capabilities": capabilities,
        "clientInfo": {"name": name, "version": env!("CARGO_PKG_VERSION")}
    })
}

pub(crate) fn agreed(result: &Value) -> Result<String> {
    let version = result
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(PROTOCOL);
    if SUPPORTED.contains(&version) {
        return Ok(version.to_string());
    }
    Err(Error::Tool(format!(
        "the server speaks MCP {version}; summonr speaks {}",
        SUPPORTED.join(", ")
    )))
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

pub(crate) enum Ask {
    Roots,
    Elicit,
    Ping,
    Unsupported,
}

pub(crate) fn ask(method: &str) -> Ask {
    match method {
        "roots/list" => Ask::Roots,
        "elicitation/create" => Ask::Elicit,
        "ping" => Ask::Ping,
        _unsupported => Ask::Unsupported,
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
