use serde::Deserialize;
use serde_json::{Value, json};

mod content;
mod messages;

use super::reply::ReplyBuilder;
use super::{Accumulate, Chat, Delta};
use crate::provider::{
    AssistantMessage, CompletionRequest, EffortField, Reasoning, Step, stream::merge_call, think,
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

    fn body(
        call: &CompletionRequest<'_>,
        encoding: &super::Encoding<'_>,
    ) -> summonr_core::error::Result<super::Wired> {
        let (reasoning, stream) = (encoding.reasoning, encoding.stream);
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
        let messages = serde_json::to_vec(&messages::Messages {
            list: call.messages,
            keep_reasoning: encoding.keep_reasoning,
            cached: encoding.cache_marks || call.traits.family == super::traits::Family::Anthropic,
            assistant_after_tool: encoding.assistant_after_tool,
        })?;
        Ok(super::Wired {
            messages: Some(messages),
            raw_tools: true,
            ..super::Wired::head(body)
        })
    }
}

#[derive(Default, Deserialize)]
struct Frame {
    #[serde(default)]
    choices: Vec<FrameChoice>,
    usage: Option<crate::provider::Usage>,
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
    finish_reason: Option<String>,
}

#[derive(Default, Deserialize)]
struct Piece {
    content: Option<content::Content>,
    refusal: Option<String>,
    #[serde(
        default,
        alias = "reasoning",
        deserialize_with = "think::reasoning_field"
    )]
    reasoning_content: Option<String>,
    #[serde(default, deserialize_with = "summonr_types::vec_or_null")]
    tool_calls: Vec<crate::provider::stream::CallDelta>,
}

#[derive(Default)]
pub(crate) struct Stream {
    reply: ReplyBuilder,
    named: usize,
    think: think::Scan,
}

impl Accumulate for Stream {
    fn new(_model: &str) -> Self {
        Self::default()
    }

    fn on(&mut self, frame: &str) -> summonr_core::error::Result<Vec<Delta>> {
        let frame = crate::provider::wire::parse::<Frame>(frame)?;
        if let Some(failure) = frame.error {
            let message = if failure.message.is_empty() {
                "the provider reported an error mid-stream".to_string()
            } else {
                failure.message
            };
            return Err(summonr_core::error::Error::provider(
                summonr_core::error::ProviderFailureKind::Server,
                message,
            ));
        }
        if let Some(counted) = frame.usage {
            self.reply.usage = counted;
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
            let ended =
                crate::provider::client::ending(crate::endpoint::Wire::Completions, &reason, "")?;
            if let Some(stop) = ended {
                self.reply.stop = stop;
            }
            self.reply.finished = true;
        }
        if let Some(part) = choice.delta.reasoning_content {
            self.thought(part, &mut deltas);
        }
        match choice.delta.content {
            Some(content::Content::Text(part)) => self.append_text(&part, &mut deltas),
            Some(content::Content::Chunks(chunks)) => {
                for chunk in &chunks {
                    if let content::Chunk::Thinking { thinking } = chunk {
                        let part: String = thinking.iter().map(content::Chunk::text).collect();
                        self.thought(part, &mut deltas);
                    } else {
                        self.append_text(chunk.text(), &mut deltas);
                    }
                }
            }
            None => {}
        }
        self.merge_calls(choice.delta.tool_calls, &mut deltas)?;
        Ok(deltas)
    }

    fn reply(&self) -> &ReplyBuilder {
        &self.reply
    }

    fn finish(mut self) -> AssistantMessage {
        let reply = &mut self.reply;
        self.think.finish(
            |chunk| reply.text.push_str(chunk),
            |chunk| reply.reasoning.push_str(chunk),
        );
        for (index, call) in self.reply.calls.iter_mut().enumerate() {
            if call.id.is_empty() {
                call.id = format!("call_{index}");
            }
            call.function.check();
        }
        self.reply.finish(None)
    }
}

impl Stream {
    fn thought(&mut self, part: String, deltas: &mut Vec<Delta>) {
        if part.is_empty() {
            return;
        }
        self.reply.reasoning.push_str(&part);
        deltas.push(Delta::Thinking(part));
    }

    fn append_text(&mut self, part: &str, deltas: &mut Vec<Delta>) {
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
            self.reply.text.push_str(&text);
            deltas.push(Delta::Text(text));
        }
    }

    fn merge_calls(
        &mut self,
        parts: Vec<crate::provider::stream::CallDelta>,
        deltas: &mut Vec<Delta>,
    ) -> summonr_core::error::Result<()> {
        for part in parts {
            if part
                .function
                .as_ref()
                .and_then(|function| function.arguments.as_deref())
                .is_some_and(|arguments| !arguments.is_empty())
            {
                deltas.push(Delta::Progress);
            }
            merge_call(&mut self.reply.calls, part)?;
        }
        while let Some(call) = self.reply.calls.get(self.named) {
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
