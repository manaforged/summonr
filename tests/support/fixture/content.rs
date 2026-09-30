use std::env;

use serde_json::{Value, json};

pub(super) fn offered() -> bool {
    env::var_os("MCP_FIXTURE_PROMPTS").is_some()
}

pub(super) fn capabilities() -> Value {
    if offered() {
        json!({
            "tools": {"listChanged": true},
            "prompts": {"listChanged": true},
            "resources": {"listChanged": true}
        })
    } else {
        json!({"tools": {"listChanged": true}})
    }
}

pub(super) fn answer(method: &str, params: &Value) -> Option<Value> {
    Some(match method {
        "prompts/list" => json!({"prompts": [{
            "name": "review",
            "description": "Review code",
            "arguments": [
                {"name": "code", "required": true},
                {"name": "lang"}
            ]
        }]}),
        "prompts/get" => {
            let code = params
                .pointer("/arguments/code")
                .and_then(Value::as_str)
                .unwrap_or("");
            let lang = params
                .pointer("/arguments/lang")
                .and_then(Value::as_str)
                .unwrap_or("any");
            json!({"messages": [{"role": "user", "content": {
                "type": "text",
                "text": format!("Review this {lang} code carefully: {code}")
            }}]})
        }
        "resources/list" => json!({"resources": [
            {"uri": "note://today", "name": "today", "mimeType": "text/plain"}
        ]}),
        "resources/templates/list" => json!({"resourceTemplates": [
            {"uriTemplate": "note://{day}", "name": "day"}
        ]}),
        "resources/read" => {
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
            json!({"contents": [{"uri": uri, "mimeType": "text/plain", "text": format!("the note kept at {uri}")}]})
        }
        _other => return None,
    })
}
