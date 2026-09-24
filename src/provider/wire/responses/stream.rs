use serde_json::Value;

use super::super::{Accumulate, Delta, blocked, parse};
use crate::error::{Error, ProviderFailureKind};
use crate::provider::stream::ARGUMENT_CAP;
use crate::provider::{
    AssistantMessage, Continuation, StopReason, ToolCall, ToolFunction, ToolKind, Usage,
};

#[derive(Default)]
pub(crate) struct Stream {
    model: String,
    text: String,
    thinking: String,
    signatures: Vec<String>,
    calls: Vec<ToolCall>,
    items: Vec<String>,
    commentary: Vec<String>,
    phase: Option<String>,
    remarks: Vec<String>,
    last: Option<String>,
    usage: Usage,
    stop: StopReason,
    done: bool,
}

impl Accumulate for Stream {
    fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            ..Self::default()
        }
    }

    fn on(&mut self, frame: &str) -> crate::error::Result<Vec<Delta>> {
        let frame = parse::<Value>(frame)?;
        self.event(&frame)
    }

    fn finish(mut self) -> AssistantMessage {
        if !self.calls.is_empty() {
            if self.stop != StopReason::Length {
                self.stop = StopReason::ToolCalls;
            }
            let signatures = std::mem::take(&mut self.signatures);
            if !signatures.is_empty()
                && let Some(call) = self.calls.first_mut()
            {
                call.continuation = Some(Continuation::Responses {
                    model: self.model.clone(),
                    signatures,
                    phase: None,
                    commentary: Vec::new(),
                });
            }
        }
        let continuation = (!self.signatures.is_empty()
            || self.phase.is_some()
            || !self.remarks.is_empty())
        .then(|| Continuation::Responses {
            model: self.model.clone(),
            signatures: std::mem::take(&mut self.signatures),
            phase: self.phase.take(),
            commentary: std::mem::take(&mut self.remarks),
        });
        AssistantMessage {
            continuation,
            content: (!self.text.is_empty()).then_some(self.text),
            tool_calls: self.calls,
            reasoning_content: (!self.thinking.is_empty()).then_some(self.thinking),
            usage: self.usage,
            stop_reason: self.stop,
        }
    }

    fn finished(&self) -> bool {
        self.done
    }
}

impl Stream {
    fn event(&mut self, frame: &Value) -> crate::error::Result<Vec<Delta>> {
        let kind = frame.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "response.output_text.delta" => return Ok(vec![self.text(frame)]),
            "response.refusal.delta" => {
                let delta = frame.get("delta").and_then(Value::as_str).unwrap_or("");
                return Err(blocked(&format!("refusal: {delta}")));
            }
            "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                let delta = frame
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.thinking.push_str(&delta);
                return Ok(vec![Delta::Thinking(delta)]);
            }
            "response.output_item.added" => {
                let item = frame.get("item").unwrap_or(&Value::Null);
                let kind = item.get("type").and_then(Value::as_str);
                if kind == Some("function_call") {
                    return Ok(self.added(item).into_iter().collect());
                }
                if kind == Some("message") {
                    self.phase(item);
                }
            }
            "response.function_call_arguments.delta" => self.arguments(frame)?,
            "response.output_item.done" => {
                if let Some(item) = frame.get("item") {
                    self.item(item)?;
                }
            }
            "response.failed" => return Err(failed(frame.pointer("/response/error"))),
            "error" => return Err(failed(frame.get("error").or(Some(frame)))),
            "response.completed" | "response.incomplete" => {
                if let Some(response) = frame.get("response") {
                    if self.text.is_empty() && self.calls.is_empty() && self.signatures.is_empty() {
                        self.value(response)?;
                        self.done = true;
                        return Ok((!self.text.is_empty())
                            .then(|| Delta::Text(self.text.clone()))
                            .into_iter()
                            .collect());
                    }
                    self.status(response)?;
                    self.done = true;
                }
            }
            _ => {}
        }
        Ok(Vec::new())
    }

    fn phase(&mut self, item: &Value) -> bool {
        match item.get("phase").and_then(Value::as_str) {
            Some("commentary") => {
                self.commentary.push(key(item).to_string());
                self.remarks.push(String::new());
                true
            }
            said => {
                self.phase = said.map(str::to_string).or(self.phase.take());
                false
            }
        }
    }

    fn text(&mut self, frame: &Value) -> Delta {
        let delta = frame.get("delta").and_then(Value::as_str).unwrap_or("");
        let item = frame.get("item_id").and_then(Value::as_str).unwrap_or("");
        if self.commentary.iter().any(|known| known == item) {
            self.thinking.push_str(delta);
            if let Some(remark) = self.remarks.last_mut() {
                remark.push_str(delta);
            }
            return Delta::Thinking(delta.to_string());
        }
        let mut text = String::new();
        if !self.text.is_empty() && self.last.as_deref() != Some(item) {
            text.push_str("\n\n");
        }
        self.last = Some(item.to_string());
        text.push_str(delta);
        self.text.push_str(&text);
        Delta::Text(text)
    }

    fn added(&mut self, item: &Value) -> Option<Delta> {
        let key = key(item).to_string();
        self.items.push(key.clone());
        let call = ToolCall {
            id: item
                .get("call_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            kind: ToolKind::Function,
            function: ToolFunction {
                name: item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                arguments: String::new(),
            },
            continuation: None,
        };
        let delta = Delta::Call(call.function.name.clone());
        self.calls.push(call);
        Some(delta)
    }

    fn arguments(&mut self, frame: &Value) -> crate::error::Result<()> {
        let key = frame.get("item_id").and_then(Value::as_str);
        let index = key.and_then(|key| self.items.iter().position(|item| item == key));
        let delta = frame.get("delta").and_then(Value::as_str);
        if let (Some(index), Some(delta)) = (index, delta)
            && let Some(call) = self.calls.get_mut(index)
        {
            call.function.arguments.push_str(delta);
            if call.function.arguments.len() > ARGUMENT_CAP {
                return Err(Error::provider(
                    ProviderFailureKind::Malformed,
                    format!("streamed tool arguments exceeded {ARGUMENT_CAP} bytes"),
                ));
            }
        }
        Ok(())
    }

    fn item(&mut self, item: &Value) -> crate::error::Result<()> {
        match item.get("type").and_then(Value::as_str) {
            Some("reasoning") => {
                if let Some(content) = item.get("encrypted_content").and_then(Value::as_str) {
                    self.signatures.push(content.to_string());
                }
            }
            Some("message") => refusal(item)?,
            Some("function_call") => {
                let key = key(item);
                if let Some(index) = self.items.iter().position(|known| known == key) {
                    if let Some(arguments) = item.get("arguments").and_then(Value::as_str)
                        && let Some(call) = self.calls.get_mut(index)
                    {
                        call.function.arguments = arguments.to_string();
                    }
                } else {
                    self.items.push(key.to_string());
                    self.calls.push(ToolCall {
                        id: item
                            .get("call_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        kind: ToolKind::Function,
                        function: ToolFunction {
                            name: item
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            arguments: item
                                .get("arguments")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        },
                        continuation: None,
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn status(&mut self, response: &Value) -> crate::error::Result<()> {
        if let Some(counted) = response.get("usage") {
            self.usage = usage(counted);
        }
        match response.get("status").and_then(Value::as_str) {
            Some("incomplete") => match response
                .pointer("/incomplete_details/reason")
                .and_then(Value::as_str)
            {
                Some("max_output_tokens") => self.stop = StopReason::Length,
                Some("content_filter") => return Err(blocked("content_filter")),
                _ => {}
            },
            Some("completed") => self.stop = StopReason::Stop,
            _ => {}
        }
        Ok(())
    }

    pub(super) fn value(&mut self, response: &Value) -> crate::error::Result<()> {
        self.status(response)?;
        for item in response
            .get("output")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match item.get("type").and_then(Value::as_str) {
                Some("reasoning") => {
                    if let Some(content) = item.get("encrypted_content").and_then(Value::as_str) {
                        self.signatures.push(content.to_string());
                    }
                }
                Some("message") => {
                    refusal(item)?;
                    let commentary = self.phase(item);
                    let target = if commentary {
                        &mut self.thinking
                    } else {
                        &mut self.text
                    };
                    if !target.is_empty() {
                        target.push_str("\n\n");
                    }
                    for part in item
                        .get("content")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        if part.get("type").and_then(Value::as_str) == Some("output_text")
                            && let Some(text) = part.get("text").and_then(Value::as_str)
                        {
                            target.push_str(text);
                            if commentary && let Some(remark) = self.remarks.last_mut() {
                                remark.push_str(text);
                            }
                        }
                    }
                }
                Some("function_call") => self.item(item)?,
                _ => {}
            }
        }
        Ok(())
    }
}

fn key(item: &Value) -> &str {
    item.get("id")
        .or_else(|| item.get("call_id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn refusal(item: &Value) -> crate::error::Result<()> {
    let parts = item.get("content").and_then(Value::as_array);
    match parts.into_iter().flatten().find_map(|part| {
        (part.get("type").and_then(Value::as_str) == Some("refusal"))
            .then(|| part.get("refusal").and_then(Value::as_str).unwrap_or(""))
    }) {
        Some(text) => Err(blocked(&format!("refusal: {text}"))),
        None => Ok(()),
    }
}

fn failed(error: Option<&Value>) -> crate::error::Error {
    let text = |key: &str| {
        error
            .and_then(|error| error.get(key))
            .and_then(Value::as_str)
            .unwrap_or("")
    };
    super::super::failed(None, text("code"), text("message"))
}

fn usage(counted: &Value) -> Usage {
    let field = |name: &str| {
        counted
            .get(name)
            .and_then(Value::as_u64)
            .unwrap_or_default()
            .min(u64::from(u32::MAX)) as u32
    };
    let detail = |outer: &str, inner: &str| {
        counted
            .get(outer)
            .and_then(|details| details.get(inner))
            .and_then(Value::as_u64)
            .unwrap_or_default()
            .min(u64::from(u32::MAX)) as u32
    };
    Usage {
        prompt_tokens: field("input_tokens"),
        completion_tokens: field("output_tokens"),
        reasoning_tokens: detail("output_tokens_details", "reasoning_tokens"),
        cached_tokens: detail("input_tokens_details", "cached_tokens"),
    }
}
