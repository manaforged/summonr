use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;

use super::{ext, frame_line, permission};
use crate::rpc;
use crate::tools::Event;

pub(super) const METHOD: &str = "session/update";

pub(super) struct Updates {
    pub(super) out: UnboundedSender<String>,
    pub(super) session: String,
    pub(super) window: Option<u32>,
    pub(super) endpoint: String,
    pub(super) ext: bool,
}

impl Updates {
    pub(super) fn emit(&self, event: &Event) {
        if self.ext {
            ext::observe(self, event);
        }
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
                cost,
                ..
            } => {
                let used = u64::from(*prompt_tokens) + u64::from(*completion_tokens);
                usage(&self.out, &self.session, self.window, used);
                if self.ext {
                    ext::usage(self, used, *cost);
                }
            }
            _other => {}
        }
        emit(&self.out, &self.session, event);
    }

    fn ask(&self, detail: &str, call: Option<&str>, persist: bool, dialog: u64) {
        let request = permission::request(&self.session, detail, call, persist, dialog);
        drop(self.out.send(frame_line(&request)));
    }
}

fn emit(out: &UnboundedSender<String>, session: &str, event: &Event) {
    let content = match event {
        Event::ToolCall {
            id,
            name,
            arguments,
        } => json!({
            "sessionUpdate": "tool_call",
            "toolCallId": id,
            "title": name,
            "kind": kind(name),
            "status": "in_progress",
            "rawInput": serde_json::from_str::<Value>(arguments).unwrap_or_else(|_invalid| json!(arguments)),
        }),
        Event::ToolResult {
            id,
            success,
            output,
            ..
        } => json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": id,
            "status": if *success { "completed" } else { "failed" },
            "content": [{"type": "content", "content": {"type": "text", "text": output}}],
        }),
        Event::Progress {
            id,
            progress,
            total,
            message,
        } => json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": id,
            "status": "in_progress",
            "content": [{"type": "content", "content": {"type": "text", "text": crate::tools::progress_text(*progress, *total, message.as_deref())}}],
        }),
        Event::Work { progress } => json!({
            "sessionUpdate": "plan",
            "entries": progress
                .todos
                .iter()
                .map(|todo| json!({"content": todo.text, "priority": "medium", "status": todo.status.acp()}))
                .collect::<Vec<_>>(),
        }),
        Event::Note { text } | Event::Aborted { reason: text, .. } => {
            chunk("agent_thought_chunk", text)
        }
        Event::Assistant { .. }
        | Event::Usage { .. }
        | Event::Unreported { .. }
        | Event::Mail { .. }
        | Event::Steered { .. }
        | Event::User { .. }
        | Event::Injected { .. }
        | Event::Cleared
        | Event::Merged { .. }
        | Event::Delta { .. }
        | Event::Reasoning { .. }
        | Event::Approval { .. }
        | Event::Elicit { .. }
        | Event::Verify { .. }
        | Event::Receipt { .. }
        | Event::Spawned { .. }
        | Event::Session { .. }
        | Event::Step { .. }
        | Event::Child { .. }
        | Event::Dismissed => return,
    };
    drop(out.send(frame_line(&update(session, content))));
}

pub(super) fn replay(out: &UnboundedSender<String>, session: &str, event: &Event) {
    let content = match event {
        Event::User { text, .. } => chunk("user_message_chunk", text),
        Event::Assistant { text } => chunk("agent_message_chunk", text),
        Event::Reasoning { text } => chunk("agent_thought_chunk", text),
        _live => return emit(out, session, event),
    };
    drop(out.send(frame_line(&update(session, content))));
}

pub(super) fn chunk(kind: &str, text: &str) -> Value {
    json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}})
}

fn usage(out: &UnboundedSender<String>, session: &str, window: Option<u32>, used: u64) {
    let Some(size) = window else {
        return;
    };
    let content = json!({"sessionUpdate": "usage_update", "used": used, "size": size});
    drop(out.send(frame_line(&update(session, content))));
}

pub(super) fn servers(
    params: &Value,
) -> std::result::Result<Vec<(String, summonr_types::mcp::Entry)>, (i64, String)> {
    let mut found = Vec::new();
    for server in params
        .get("mcpServers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let stdio = server.get("type").is_none();
        let name = server.get("name").and_then(Value::as_str);
        let command = server.get("command").and_then(Value::as_str);
        let (Some(name), Some(command), true) = (name, command, stdio) else {
            return Err((
                super::codes::INVALID,
                format!("mcpServers takes stdio servers with a name and command, not {server}"),
            ));
        };
        let strings = |key: &str| {
            server
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        };
        let entry = summonr_types::mcp::Entry {
            transport: Some(summonr_types::mcp::Transport::Stdio),
            command: Some(command.to_string()),
            args: strings("args")
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
            env: strings("env")
                .filter_map(|pair| {
                    let key = pair.get("name")?.as_str()?;
                    Some((key.to_string(), pair.get("value")?.as_str()?.to_string()))
                })
                .collect(),
            ..summonr_types::mcp::Entry::default()
        };
        found.push((name.to_string(), entry));
    }
    Ok(found)
}

pub(super) fn update(session: &str, content: Value) -> Value {
    rpc::notification(METHOD, json!({"sessionId": session, "update": content}))
}

fn kind(name: &str) -> &'static str {
    crate::tools::Builtin::live(name).map_or("other", crate::tools::Builtin::acp_kind)
}

pub(super) fn prompt_text(params: &Value) -> String {
    params
        .get("prompt")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(block)
        .collect::<Vec<_>>()
        .join("\n")
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
