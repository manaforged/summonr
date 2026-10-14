use super::super::{Accumulate, Delta, blocked, parse};
use super::frame::{Failure, Frame, FrameKind, Item, PartShape, Response, Shape};
use crate::provider::message::Checked;
use crate::provider::stream::append_arguments;
use crate::provider::{
    AssistantMessage, Continuation, Partial, StopReason, ToolCall, ToolFunction, ToolKind, Usage,
};

#[cfg(test)]
use serde_json::Value;

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
    reasoning: bool,
}

impl Accumulate for Stream {
    fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            ..Self::default()
        }
    }

    fn on(&mut self, frame: &str) -> crate::error::Result<Vec<Delta>> {
        let Some(frame) = parse::<Option<Frame<'_>>>(frame)? else {
            return Ok(Vec::new());
        };
        self.event(&frame)
    }

    fn finish(mut self) -> AssistantMessage {
        for call in &mut self.calls {
            call.function.check();
        }
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

    fn thinking(&self) -> bool {
        self.reasoning
    }

    fn spill(&mut self, partial: &mut Partial) {
        partial.text.push_str(&self.text);
        partial.reasoning.push_str(&self.thinking);
        if self.usage != Usage::default() {
            partial.usage = Some(self.usage.clone());
        }
    }
}

impl Stream {
    fn event(&mut self, frame: &Frame<'_>) -> crate::error::Result<Vec<Delta>> {
        match frame.kind {
            FrameKind::TextDelta => return Ok(vec![self.text(frame)]),
            FrameKind::RefusalDelta => {
                return Err(blocked(&format!("refusal: {}", frame.delta)));
            }
            FrameKind::ReasoningDelta | FrameKind::SummaryDelta => {
                let delta = frame.delta.to_string();
                self.thinking.push_str(&delta);
                return Ok(vec![Delta::Thinking(delta)]);
            }
            FrameKind::ItemAdded => return Ok(self.opened(frame.item.as_ref())),
            FrameKind::ArgumentsDelta => return self.arguments(frame),
            FrameKind::ItemDone => {
                if let Some(item) = &frame.item {
                    self.item(item)?;
                }
                return Ok(vec![Delta::Progress]);
            }
            FrameKind::Failed => {
                return Err(failed(
                    frame
                        .response
                        .as_ref()
                        .and_then(|response| response.error.as_ref()),
                    frame,
                ));
            }
            FrameKind::Error => return Err(failed(frame.error.as_ref(), frame)),
            FrameKind::Completed | FrameKind::Incomplete => {
                if let Some(response) = &frame.response {
                    return self.completed(response);
                }
            }
            FrameKind::Other => {}
        }
        Ok(Vec::new())
    }

    fn opened(&mut self, item: Option<&Item<'_>>) -> Vec<Delta> {
        match item {
            Some(item) if item.shape == Shape::FunctionCall => vec![self.added(item)],
            Some(item) if item.shape == Shape::Reasoning => {
                self.reasoning = true;
                vec![Delta::Progress]
            }
            Some(item) if item.shape == Shape::Message => {
                self.phase(item);
                vec![Delta::Progress]
            }
            _ => vec![Delta::Progress],
        }
    }

    fn completed(&mut self, response: &Response<'_>) -> crate::error::Result<Vec<Delta>> {
        self.done = true;
        if self.text.is_empty() && self.calls.is_empty() && self.signatures.is_empty() {
            self.value(response)?;
            return Ok((!self.text.is_empty())
                .then(|| Delta::Text(self.text.clone()))
                .into_iter()
                .collect());
        }
        self.status(response)?;
        Ok(Vec::new())
    }

    fn phase(&mut self, item: &Item<'_>) -> bool {
        match item.phase.as_deref() {
            Some("commentary") => {
                self.commentary.push(item.key().to_string());
                self.remarks.push(String::new());
                true
            }
            said => {
                self.phase = said.map(str::to_string).or(self.phase.take());
                false
            }
        }
    }

    fn text(&mut self, frame: &Frame<'_>) -> Delta {
        let delta: &str = &frame.delta;
        let item = frame.item_id.as_deref().unwrap_or("");
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

    fn added(&mut self, item: &Item<'_>) -> Delta {
        self.items.push(item.key().to_string());
        let call = called(item, String::new());
        let delta = Delta::Call(call.function.name.clone());
        self.calls.push(call);
        delta
    }

    fn arguments(&mut self, frame: &Frame<'_>) -> crate::error::Result<Vec<Delta>> {
        let key = frame.item_id.as_deref();
        let index = key.and_then(|key| self.items.iter().position(|item| item == key));
        if let Some(index) = index
            && let Some(call) = self.calls.get_mut(index)
        {
            append_arguments(&mut call.function.arguments, &frame.delta)?;
        }
        Ok((!frame.delta.is_empty())
            .then_some(Delta::Progress)
            .into_iter()
            .collect())
    }

    fn item(&mut self, item: &Item<'_>) -> crate::error::Result<()> {
        match item.shape {
            Shape::Reasoning => {
                self.reasoning = false;
                if let Some(content) = item.encrypted_content.as_deref() {
                    self.signatures.push(content.to_string());
                }
            }
            Shape::Message => refusal(item)?,
            Shape::FunctionCall => {
                let key = item.key();
                if let Some(index) = self.items.iter().position(|known| known == key) {
                    if let Some(arguments) = item.arguments.as_deref()
                        && let Some(call) = self.calls.get_mut(index)
                    {
                        call.function.arguments = arguments.to_string();
                    }
                } else {
                    self.items.push(key.to_string());
                    let arguments = item.arguments.as_deref().unwrap_or_default().to_string();
                    self.calls.push(called(item, arguments));
                }
            }
            Shape::Other => {}
        }
        Ok(())
    }

    fn status(&mut self, response: &Response<'_>) -> crate::error::Result<()> {
        if let Some(counted) = &response.usage {
            self.usage = counted.usage();
        }
        match response.status.as_deref() {
            Some("incomplete") => match response
                .incomplete_details
                .as_ref()
                .and_then(|details| details.reason.as_deref())
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

    #[cfg(test)]
    pub(super) fn whole(&mut self, response: &Value) -> crate::error::Result<()> {
        let text = response.to_string();
        self.value(&parse::<Response<'_>>(&text)?)
    }

    fn value(&mut self, response: &Response<'_>) -> crate::error::Result<()> {
        self.status(response)?;
        let Some(output) = response.output else {
            return Ok(());
        };
        for item in parse::<Vec<Item<'_>>>(output.get())? {
            match item.shape {
                Shape::Message => self.message(&item)?,
                Shape::Reasoning | Shape::FunctionCall => self.item(&item)?,
                Shape::Other => {}
            }
        }
        Ok(())
    }

    fn message(&mut self, item: &Item<'_>) -> crate::error::Result<()> {
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
        for part in item.parts() {
            if part.shape == PartShape::OutputText
                && let Some(text) = part.text.as_deref()
            {
                target.push_str(text);
                if commentary && let Some(remark) = self.remarks.last_mut() {
                    remark.push_str(text);
                }
            }
        }
        Ok(())
    }
}

fn called(item: &Item<'_>, arguments: String) -> ToolCall {
    ToolCall {
        id: item.call_id.as_deref().unwrap_or_default().to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: item.name.as_deref().unwrap_or_default().to_string(),
            arguments,
            checked: Checked::Unchecked,
        },
        continuation: None,
    }
}

fn refusal(item: &Item<'_>) -> crate::error::Result<()> {
    match item.parts().iter().find_map(|part| {
        (part.shape == PartShape::Refusal).then(|| part.refusal.as_deref().unwrap_or(""))
    }) {
        Some(text) => Err(blocked(&format!("refusal: {text}"))),
        None => Ok(()),
    }
}

fn failed(error: Option<&Failure<'_>>, frame: &Frame<'_>) -> crate::error::Error {
    let (code, message) = match error {
        Some(error) => (error.code.as_ref(), error.message.as_deref()),
        None if frame.kind == FrameKind::Error => (frame.code.as_ref(), frame.message.as_deref()),
        None => (None, None),
    };
    super::super::failed(
        None,
        code.and_then(serde_json::Value::as_str).unwrap_or(""),
        message.unwrap_or(""),
    )
}
