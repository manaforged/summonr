use serde_json::{Value, json};

use super::*;
use crate::durable::id::Call;
use crate::hooks::{Dialect, Hook, HookEvent, Permission, Verdict};

mod life;

impl<P: Provider> Loop<P> {
    pub(super) async fn hook(
        &self,
        event: HookEvent,
        subject: Option<&str>,
        payload: Value,
    ) -> Verdict {
        self.hook_where(event, subject, payload, |_| true).await
    }

    async fn hook_where(
        &self,
        event: HookEvent,
        subject: Option<&str>,
        mut payload: Value,
        keep: impl Fn(&Hook) -> bool,
    ) -> Verdict {
        let mut verdict = Verdict::default();
        let hooks: Vec<&Hook> = self
            .hooks
            .of(event)
            .iter()
            .filter(|hook| hook.applies(subject) && keep(hook))
            .collect();
        if hooks.is_empty() {
            return verdict;
        }
        let session = self.session_id();
        let transcript = self.session.path();
        if let Some(object) = payload.as_object_mut() {
            object.insert("event".to_string(), json!(event.key()));
            object.insert("session".to_string(), json!(session));
            object.insert("workspace".to_string(), json!(self.workspace));
            object.insert("transcript".to_string(), json!(transcript));
        }
        let permission = if self.is_approval() {
            Permission::Ask
        } else {
            Permission::Auto
        };
        let call = crate::hooks::Call {
            event,
            workspace: &self.workspace,
            session: &session,
            transcript,
            permission,
            payload: &payload,
        };
        for hook in hooks {
            let heard = crate::hooks::run(hook, &call).await;
            let stop = event == HookEvent::PreTool && heard.blocked.is_some();
            verdict.merge(heard);
            if stop {
                break;
            }
        }
        verdict
    }

    pub(super) fn hook_notes<'a>(
        &mut self,
        event: HookEvent,
        notes: impl IntoIterator<Item = &'a String>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        for note in notes {
            self.push_event(events, emit, note_event(event, note))?;
        }
        Ok(())
    }

    pub(super) async fn tool_text(
        &mut self,
        call: &ToolCall,
        success: bool,
        text: String,
    ) -> String {
        let mut text = crate::secret::shield::scrub(&text, &self.workspace);
        if self.hooks.of(HookEvent::PostTool).is_empty()
            && self.hooks.of(HookEvent::ToolFailure).is_empty()
        {
            return text;
        }
        let (tool, input) = seen(call);
        let payload = json!({
            "id": call.id,
            "tool": tool,
            "arguments": input,
            "success": success,
            "output": text,
        });
        let mut heard = self
            .hook_where(HookEvent::PostTool, Some(&tool), payload.clone(), |hook| {
                success || hook.dialect == Dialect::Summonr
            })
            .await;
        if !success {
            heard.merge(
                self.hook(HookEvent::ToolFailure, Some(&tool), payload)
                    .await,
            );
        }
        if let Some(note) = heard.note() {
            text = format!("{text}\n[post_tool hook: {note}]");
        }
        text
    }

    pub(super) async fn pre_tool(&self, call: &ToolCall) -> Verdict {
        let (tool, input) = seen(call);
        let payload = json!({
            "id": call.id,
            "tool": tool,
            "arguments": input,
        });
        self.hook(HookEvent::PreTool, Some(&tool), payload).await
    }

    pub(super) async fn screen(
        &mut self,
        planned: Vec<Call>,
        calls: Vec<ToolCall>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<(Vec<Call>, Vec<ToolCall>)> {
        if self.hooks.of(HookEvent::PreTool).is_empty() {
            return Ok((planned, calls));
        }
        let mut kept = (Vec::new(), Vec::new());
        for (planned, mut call) in planned.into_iter().zip(calls) {
            let heard = self.pre_tool(&call).await;
            match self
                .resolve_pre_tool(heard, &mut call, events, emit)
                .await?
            {
                Some(reason) => {
                    let blocked = Err(blocked(&reason));
                    self.finish_call(planned, call, (blocked, 0), events, emit)
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

    async fn resolve_pre_tool(
        &mut self,
        mut heard: Verdict,
        call: &mut ToolCall,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<String>> {
        self.hook_notes(
            HookEvent::PreTool,
            heard.notes.iter().chain(&heard.halted),
            events,
            emit,
        )?;
        if heard.blocked.is_none()
            && let Some(reason) = heard.ask.take()
        {
            let label = crate::mcp::call_label(&call.function.name, &call.function.arguments);
            let detail = format!("a pre_tool hook asks before {label}: {reason}");
            if self.approve(detail, false, events, emit).await? == GrantAnswer::Deny {
                heard.blocked = Some(if reason.is_empty() {
                    "the user declined the call".to_string()
                } else {
                    reason
                });
            }
        }
        if heard.blocked.is_some() {
            return Ok(Some(heard.feedback().join("; ")));
        }
        self.hook_notes(HookEvent::PreTool, &heard.context, events, emit)?;
        let Some(input) = heard.input else {
            return Ok(None);
        };
        let text = if crate::mcp::effective(&call.function.name, &call.function.arguments).is_some()
        {
            "updatedInput is ignored for a call routed through mcp".to_string()
        } else {
            call.function.arguments = input.to_string();
            format!("rewrote the {} call's input", call.function.name)
        };
        self.push_event(events, emit, note_event(HookEvent::PreTool, &text))?;
        Ok(None)
    }
}

pub(super) fn note_event(event: HookEvent, text: &str) -> Event {
    Event::Note {
        text: format!("{} hook: {text}", event.key()),
    }
}

pub(super) fn context_note(context: &[String]) -> Option<String> {
    (!context.is_empty()).then(|| format!("Harness: hook context:\n{}", context.join("\n\n")))
}

pub(super) fn annotate(input: &mut UserInput, note: &str) {
    if input.source.is_none() {
        input.source = Some(crate::provider::Source {
            text: input.text.clone(),
            images: input.images.len(),
        });
    }
    input.text = crate::session::decorate(note, &input.text);
}

pub(super) fn blocked(reason: &str) -> Error {
    Error::Denied(format!("the pre_tool hook blocked this call: {reason}"))
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
