use serde_json::{Value, json};

use super::super::{Message, ToolDefinition};
use super::frame::Rpc;

pub(super) const SERVER: &str = "summonr";

pub(super) fn mcp(message: &Value, tools: &[ToolDefinition]) -> Value {
    let id = &message["id"];
    if id.is_null() {
        return json!({"jsonrpc": "2.0", "result": {}});
    }
    match serde_json::from_value::<Rpc>(message.clone()) {
        Ok(Rpc::Initialize { id, params }) => crate::rpc::response(
            &id,
            json!({
                "protocolVersion": params["protocolVersion"],
                "capabilities": {"tools": {}},
                "serverInfo": {"name": SERVER, "version": env!("CARGO_PKG_VERSION")}
            }),
        ),
        Ok(Rpc::ToolsList { id }) => crate::rpc::response(
            &id,
            json!({"tools": tools.iter().map(listed).collect::<Vec<_>>()}),
        ),
        Ok(Rpc::ToolsCall { .. } | Rpc::Other) | Err(_) => {
            crate::rpc::failure(id, -32601, "method not found")
        }
    }
}

fn listed(tool: &ToolDefinition) -> Value {
    json!({
        "name": tool.function.name,
        "description": tool.function.description,
        "inputSchema": tool.function.parameters
    })
}

pub(super) fn called(id: &Value, result: Option<&Message>) -> Value {
    let Some(message) = result else {
        return crate::rpc::response(
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
    crate::rpc::response(id, json!({"content": content, "isError": message.failed}))
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
