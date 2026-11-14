use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;

use super::frame_line;
use crate::rpc;
use crate::tools::Event;

pub(super) fn emit(out: &UnboundedSender<String>, session: &str, event: &Event) {
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
        Event::Note { text } | Event::Aborted { reason: text, .. } => json!({
            "sessionUpdate": "agent_thought_chunk",
            "content": {"type": "text", "text": text},
        }),
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

pub(super) fn update(session: &str, content: Value) -> Value {
    rpc::notification(
        "session/update",
        json!({"sessionId": session, "update": content}),
    )
}

fn kind(name: &str) -> &'static str {
    crate::tools::Builtin::live(name).map_or("other", crate::tools::Builtin::acp_kind)
}
