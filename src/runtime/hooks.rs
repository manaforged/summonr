use serde_json::{Value, json};

use super::*;
use crate::durable::id::Call;
use crate::hooks::{HookEvent, Verdict};

pub(super) struct Heard {
    pub(super) blocked: Option<String>,
    pub(super) notes: Vec<String>,
}

impl Heard {
    pub(super) fn note(self) -> Option<String> {
        let notes: Vec<String> = self.blocked.into_iter().chain(self.notes).collect();
        (!notes.is_empty()).then(|| notes.join("; "))
    }
}

impl<P: Provider> Loop<P> {
    pub(super) async fn hook(
        &self,
        event: HookEvent,
        tool: Option<&str>,
        mut payload: Value,
    ) -> Heard {
        let mut heard = Heard {
            blocked: None,
            notes: Vec::new(),
        };
        if self.hooks.of(event).is_empty() {
            return heard;
        }
        let session = self.session_id();
        if let Some(object) = payload.as_object_mut() {
            object.insert("session".to_string(), json!(session));
            object.insert("workspace".to_string(), json!(self.workspace));
        }
        let call = crate::hooks::Call {
            event,
            workspace: &self.workspace,
            session: &session,
            payload: &payload,
        };
        for hook in self
            .hooks
            .of(event)
            .iter()
            .filter(|hook| hook.applies(tool))
        {
            match crate::hooks::run(hook, &call).await {
                Verdict::Allow => {}
                Verdict::Block(reason) if event == HookEvent::PreTool => {
                    heard.blocked = Some(reason);
                    return heard;
                }
                Verdict::Block(reason) | Verdict::Note(reason) => heard.notes.push(reason),
            }
        }
        heard
    }

    pub(super) async fn tool_text(
        &mut self,
        call: &ToolCall,
        success: bool,
        text: String,
    ) -> String {
        let mut text = crate::secret::shield::scrub(&text, &self.workspace, self.shield_epoch);
        if self.hooks.of(HookEvent::PostTool).is_empty() {
            return text;
        }
        let (tool, input) = seen(call);
        let payload = json!({
            "event": "post_tool",
            "id": call.id,
            "tool": tool,
            "arguments": input,
            "success": success,
            "output": text,
        });
        if let Some(note) = self
            .hook(HookEvent::PostTool, Some(&tool), payload)
            .await
            .note()
        {
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
        if self.hooks.of(HookEvent::PreTool).is_empty() {
            return Ok((planned, calls));
        }
        let mut kept = (Vec::new(), Vec::new());
        for (planned, call) in planned.into_iter().zip(calls) {
            let (tool, input) = seen(&call);
            let payload = json!({
                "event": "pre_tool",
                "id": call.id,
                "tool": tool,
                "arguments": input,
            });
            let heard = self.hook(HookEvent::PreTool, Some(&tool), payload).await;
            for note in &heard.notes {
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("pre_tool hook: {note}"),
                    },
                )?;
            }
            match heard.blocked {
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

fn seen(call: &ToolCall) -> (String, Value) {
    crate::mcp::effective(&call.function.name, &call.function.arguments).unwrap_or_else(|| {
        (
            call.function.name.clone(),
            arguments(&call.function.arguments),
        )
    })
}
