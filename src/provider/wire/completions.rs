use serde::Deserialize;
use serde_json::{Value, json};

mod said;

use super::{Accumulate, Chat, Delta};
use crate::provider::{
    AssistantMessage, CompletionRequest, EffortField, Partial, Reasoning, Step, StopReason,
    ToolCall, Usage, stream::merge_call, think,
};

pub(crate) struct Wire;

fn effort(body: &mut Value, call: &CompletionRequest<'_>, reasoning: &Reasoning) {
    let plan = reasoning.plan(call.traits);
    let step = plan.step(call.effort);
    if call.traits.preserved && !matches!(step, Step::Off(_)) {
        body["thinking"] = json!({"type": "enabled", "clear_thinking": false});
    }
    match step {
        Step::Default => {}
        Step::Level(level) => match plan.field {
            EffortField::ReasoningEffort => body["reasoning_effort"] = json!(level.as_str()),
            EffortField::ReasoningObject => body["reasoning"] = json!({"effort": level.as_str()}),
        },
        Step::Off(off) => {
            if let Some((key, value)) = off.fragment() {
                body[key] = value;
            }
        }
    }
}

impl Chat for Wire {
    type Stream = Stream;

    fn path(_model: &str, _stream: bool) -> String {
        "/chat/completions".to_string()
    }

    fn sentinel() -> Option<&'static str> {
        Some("[DONE]")
    }

    fn body(
        call: &CompletionRequest<'_>,
        reasoning: &Reasoning,
        _cache: crate::endpoint::CacheTtl,
        stream: bool,
    ) -> crate::error::Result<super::Wired> {
        let mut body = json!({
            "model": call.model,
            "stream": stream,
        });
        if let Some(max) = call.max_output_tokens {
            body["max_tokens"] = json!(max);
        }
        if !call.tools.is_empty() {
            if call.tools_json.is_some() {
                body["tools"] = Value::Null;
            } else if let Ok(tools) = serde_json::to_value(call.tools) {
                body["tools"] = tools;
            }
        }
        if stream {
            body["stream_options"] = json!({"include_usage": true});
            if !call.tools.is_empty() && call.traits.tool_stream {
                body["tool_stream"] = json!(true);
            }
        }
        effort(&mut body, call, reasoning);
        Ok(super::Wired::head(body))
    }

    #[cfg(test)]
    fn parse(_model: &str, body: &Value) -> AssistantMessage {
        let reply: Reply = serde_json::from_value(body.clone()).unwrap_or_default();
        let mut message = reply
            .choices
            .into_iter()
            .next()
            .map(|choice| {
                let mut message = choice.message;
                if let Some(reason) = choice.finish_reason {
                    message.stop_reason = reason;
                }
                message
            })
            .unwrap_or_default();
        if let Some(counted) = reply.usage {
            message.usage = counted;
        }
        message
    }
}

#[cfg(test)]
#[derive(Default, Deserialize)]
struct Reply {
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[cfg(test)]
#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    message: AssistantMessage,
    finish_reason: Option<StopReason>,
}

#[derive(Default, Deserialize)]
struct Frame {
    #[serde(default)]
    choices: Vec<FrameChoice>,
    usage: Option<Usage>,
    error: Option<Failure>,
}

#[derive(Deserialize)]
struct Failure {
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct FrameChoice {
    #[serde(default)]
    delta: Piece,
    finish_reason: Option<said::Finish>,
}

#[derive(Default, Deserialize)]
struct Piece {
    content: Option<said::Said>,
    refusal: Option<String>,
    #[serde(
        default,
        alias = "reasoning",
        deserialize_with = "think::reasoning_field"
    )]
    reasoning_content: Option<String>,
    #[serde(default, deserialize_with = "think::vec_or_null")]
    tool_calls: Vec<crate::provider::stream::CallDelta>,
}

#[derive(Default)]
pub(crate) struct Stream {
    text: String,
    reasoning: String,
    calls: Vec<ToolCall>,
    named: usize,
    think: think::Scan,
    usage: Usage,
    stop: StopReason,
    finished: bool,
}

impl Accumulate for Stream {
    fn new(_model: &str) -> Self {
        Self::default()
    }

    fn on(&mut self, frame: &str) -> crate::error::Result<Vec<Delta>> {
        let frame = crate::provider::wire::parse::<Frame>(frame)?;
        if let Some(failure) = frame.error {
            let message = if failure.message.is_empty() {
                "the provider reported an error mid-stream".to_string()
            } else {
                failure.message
            };
            return Err(crate::error::Error::provider(
                crate::error::ProviderFailureKind::Server,
                message,
            ));
        }
        if let Some(counted) = frame.usage {
            self.usage = counted;
        }
        let mut deltas = Vec::new();
        let Some(choice) = frame.choices.into_iter().next() else {
            return Ok(deltas);
        };
        if let Some(refusal) = choice.delta.refusal.filter(|text| !text.is_empty()) {
            return Err(crate::provider::wire::blocked(&format!(
                "refusal: {refusal}"
            )));
        }
        if let Some(reason) = choice.finish_reason {
            self.stop = reason.stop()?;
            self.finished = true;
        }
        if let Some(part) = choice.delta.reasoning_content {
            self.thought(part, &mut deltas);
        }
        match choice.delta.content {
            Some(said::Said::Text(part)) => self.said(&part, &mut deltas),
            Some(said::Said::Chunks(chunks)) => {
                for chunk in &chunks {
                    if let said::Chunk::Thinking { thinking } = chunk {
                        let part: String = thinking.iter().map(said::Chunk::text).collect();
                        self.thought(part, &mut deltas);
                    } else {
                        self.said(chunk.text(), &mut deltas);
                    }
                }
            }
            None => {}
        }
        self.merge_calls(choice.delta.tool_calls, &mut deltas)?;
        Ok(deltas)
    }

    fn finished(&self) -> bool {
        self.finished
    }

    fn spill(&mut self, partial: &mut Partial) {
        partial.text.push_str(&self.text);
        partial.reasoning.push_str(&self.reasoning);
    }

    fn finish(mut self) -> AssistantMessage {
        self.think.finish(
            |chunk| self.text.push_str(chunk),
            |chunk| self.reasoning.push_str(chunk),
        );
        for (index, call) in self.calls.iter_mut().enumerate() {
            if call.id.is_empty() {
                call.id = format!("call_{index}");
            }
            call.function.check();
        }
        AssistantMessage {
            content: (!self.text.is_empty()).then_some(self.text),
            tool_calls: self.calls,
            reasoning_content: (!self.reasoning.is_empty()).then_some(self.reasoning),
            usage: self.usage,
            stop_reason: self.stop,
            continuation: None,
        }
    }
}

impl Stream {
    fn thought(&mut self, part: String, deltas: &mut Vec<Delta>) {
        if part.is_empty() {
            return;
        }
        self.reasoning.push_str(&part);
        deltas.push(Delta::Thinking(part));
    }

    fn said(&mut self, part: &str, deltas: &mut Vec<Delta>) {
        if part.is_empty() {
            return;
        }
        let mut text = String::new();
        let mut thought = String::new();
        self.think.push(
            part,
            |chunk| text.push_str(chunk),
            |chunk| thought.push_str(chunk),
        );
        self.thought(thought, deltas);
        if !text.is_empty() {
            self.text.push_str(&text);
            deltas.push(Delta::Text(text));
        }
    }

    fn merge_calls(
        &mut self,
        parts: Vec<crate::provider::stream::CallDelta>,
        deltas: &mut Vec<Delta>,
    ) -> crate::error::Result<()> {
        for part in parts {
            if part
                .function
                .as_ref()
                .and_then(|function| function.arguments.as_deref())
                .is_some_and(|arguments| !arguments.is_empty())
            {
                deltas.push(Delta::Progress);
            }
            merge_call(&mut self.calls, part)?;
        }
        while let Some(call) = self.calls.get(self.named) {
            if call.function.name.is_empty() {
                break;
            }
            deltas.push(Delta::Call(call.function.name.clone()));
            self.named += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "completions_tests.rs"]
mod tests;
