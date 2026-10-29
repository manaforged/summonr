use serde_json::{Value, json};

use super::super::Message;
use super::frame::CallParams;
use crate::rpc::{self, Ask, Frame};

pub(super) const SERVER: &str = "summonr";

pub(super) fn mcp(message: &Value, tools: &[Value]) -> Value {
    let Ok(Frame::Request { id, method, params }) = rpc::frame(message.clone()) else {
        return json!({"jsonrpc": "2.0", "result": {}});
    };
    let reply = match rpc::ask(&method) {
        Ask::Initialize => Ok(json!({
            "protocolVersion": params["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": SERVER, "version": env!("CARGO_PKG_VERSION")}
        })),
        Ask::ToolsList => Ok(json!({"tools": tools})),
        Ask::Roots
        | Ask::Elicit
        | Ask::Ping
        | Ask::Authenticate
        | Ask::SessionNew
        | Ask::SessionPrompt
        | Ask::SessionCancel
        | Ask::ToolsCall
        | Ask::Unsupported => Err(rpc::unsupported(&method)),
    };
    rpc::answer(&id, reply)
}

pub(super) fn call(message: &Value) -> Option<(Value, CallParams)> {
    let Ok(Frame::Request { id, method, params }) = rpc::frame(message.clone()) else {
        return None;
    };
    if rpc::ask(&method) != Ask::ToolsCall {
        return None;
    }
    serde_json::from_value(params)
        .ok()
        .map(|params| (id, params))
}

pub(super) fn called(id: &Value, result: Option<&Message>) -> Value {
    let Some(message) = result else {
        return rpc::response(
            id,
            json!({
                "content": [{"type": "text", "text": "summonr did not run this call"}],
                "isError": true
            }),
        );
    };
    let mut content = vec![json!({"type": "text", "text": message.content})];
    content.extend(
        message.images.iter().map(
            |image| json!({"type": "image", "data": image.data, "mimeType": image.media_type}),
        ),
    );
    rpc::response(id, json!({"content": content, "isError": message.failed}))
}

pub(super) fn user(message: &Message) -> Value {
    let mut content = vec![json!({"type": "text", "text": message.content})];
    content.extend(message.images.iter().map(|image| {
        json!({
            "type": "image",
            "source": {"type": "base64", "media_type": image.media_type, "data": image.data}
        })
    }));
    json!({
        "type": "user",
        "message": {"role": "user", "content": content},
        "parent_tool_use_id": null,
        "session_id": "default"
    })
}

pub(super) fn reply(id: &str, response: Value) -> Value {
    json!({
        "type": "control_response",
        "response": {"subtype": "success", "request_id": id, "response": response}
    })
}

pub(super) fn refusal(id: &str) -> Value {
    json!({
        "type": "control_response",
        "response": {
            "subtype": "error",
            "request_id": id,
            "error": "summonr does not handle this request"
        }
    })
}
