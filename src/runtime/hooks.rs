use serde_json::{Value, json};

use super::*;
use crate::durable::id::Call;

impl<P: Provider> Loop<P> {
    pub(super) async fn hook(&self, command: Option<String>, mut payload: Value) -> Option<String> {
        let command = command?;
        if let Some(object) = payload.as_object_mut() {
            object.insert("session".to_string(), json!(self.session_id()));
            object.insert("workspace".to_string(), json!(self.workspace));
        }
        crate::hooks::run(&command, &self.workspace, &payload)
            .await
            .err()
    }

    pub(super) async fn tool_text(
        &mut self,
        call: &ToolCall,
        success: bool,
        text: String,
    ) -> String {
        let mut text = text;
        let payload = json!({
            "event": "post_tool",
            "tool": call.function.name,
            "arguments": arguments(&call.function.arguments),
            "success": success,
            "output": text,
        });
        if let Some(note) = self.hook(self.hooks.post_tool.clone(), payload).await {
            text = format!("{text}\n[post_tool hook: {note}]");
        }
        text
    }

    pub(super) async fn screen(
        &mut self,
        planned: Vec<Call>,
        calls: Vec<ToolCall>,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<(Vec<Call>, Vec<ToolCall>)> {
        if self.hooks.pre_tool.is_none() {
            return Ok((planned, calls));
        }
        let mut kept = (Vec::new(), Vec::new());
        for (planned, call) in planned.into_iter().zip(calls) {
            let payload = json!({
                "event": "pre_tool",
                "tool": call.function.name,
                "arguments": arguments(&call.function.arguments),
            });
            match self.hook(self.hooks.pre_tool.clone(), payload).await {
                Some(reason) => {
                    let blocked = Err(Error::Denied(format!(
                        "the pre_tool hook blocked this call: {reason}"
                    )));
                    self.finish_call(planned, call, (blocked, 0), messages, events, emit)
                        .await?;
                }
                None => {
                    kept.0.push(planned);
                    kept.1.push(call);
                }
            }
        }
        Ok(kept)
    }
}

pub(super) fn arguments(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_invalid| json!(text))
}
