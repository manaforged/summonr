use serde_json::json;

use super::super::wire::Delta;
use super::super::{
    AssistantMessage, Client, CompletionRequest, Message, Partial, Role, StopReason, ToolCall,
    ToolFunction, ToolKind, Usage,
};
use super::bridge;
use super::frame::{Block, Control, Finished, Frame, LimitStatus, Piece, Rpc, Streamed};
use super::run::{Call, Run};
use crate::error::{Error, ProviderFailureKind, Result};

pub(in crate::provider) enum Step {
    Paused(AssistantMessage),
    Done(AssistantMessage),
}

impl Run {
    pub(super) fn abandoned(&self, messages: &[Message]) -> bool {
        let answered: std::collections::BTreeSet<&str> = messages
            .iter()
            .filter(|message| message.role == Role::Tool)
            .filter_map(|message| message.tool_call_id.as_deref())
            .collect();
        self.pending
            .iter()
            .any(|call| !answered.contains(call.id.as_str()))
    }

    pub(super) async fn answer(&mut self, messages: &[Message]) -> Result<()> {
        for call in std::mem::take(&mut self.pending) {
            let result = messages.iter().rev().find(|message| {
                message.role == Role::Tool && message.tool_call_id.as_deref() == Some(&call.id)
            });
            let reply = bridge::called(&call.rpc, result);
            self.send(&bridge::reply(
                &call.request,
                json!({"mcp_response": reply}),
            ))
            .await?;
        }
        let split = messages
            .iter()
            .rposition(|message| message.role != Role::User)
            .map_or(0, |at| at + 1);
        for message in &messages[split..] {
            self.send(&bridge::user(message)).await?;
        }
        Ok(())
    }

    pub(super) async fn next(
        &mut self,
        client: &Client,
        request: &CompletionRequest<'_>,
        partial: &mut Partial,
    ) -> Result<Step> {
        loop {
            if !self.pending.is_empty() && !self.reader.buffer().contains(&b'\n') {
                return Ok(Step::Paused(self.paused()));
            }
            match self.event().await? {
                Frame::ControlRequest {
                    request_id,
                    request,
                } => {
                    if let Some(call) = self.control(request_id, request).await? {
                        self.pending.push(call);
                    }
                }
                Frame::StreamEvent {
                    event: Streamed::ContentBlockDelta { delta },
                    parent_tool_use_id: None,
                } => self.delta(client, request, partial, delta)?,
                Frame::Assistant {
                    message,
                    parent_tool_use_id: None,
                } => {
                    self.seen = true;
                    self.meter(&message.id, message.usage);
                    self.collect(message.content);
                }
                Frame::RateLimitEvent { rate_limit_info } => {
                    self.rejected |= rate_limit_info.status == LimitStatus::Rejected;
                }
                Frame::Result(finished) => {
                    self.seen = true;
                    return self.finish(*finished).map(Step::Done);
                }
                Frame::StreamEvent { .. }
                | Frame::Assistant { .. }
                | Frame::ControlResponse { .. }
                | Frame::Other => {}
            }
        }
    }

    pub(super) async fn control(&mut self, id: String, request: Control) -> Result<Option<Call>> {
        match request {
            Control::McpMessage { message } => {
                if let Ok(Rpc::ToolsCall { id: rpc, params }) =
                    serde_json::from_value::<Rpc>(message.clone())
                {
                    return Ok(Some(Call {
                        request: id,
                        rpc,
                        id: params.meta.tool_use_id,
                        name: params.name,
                        arguments: params.arguments,
                    }));
                }
                let reply = bridge::mcp(&message, &self.tools);
                self.send(&bridge::reply(&id, json!({"mcp_response": reply})))
                    .await?;
            }
            Control::CanUseTool { input } => {
                self.send(&bridge::reply(
                    &id,
                    json!({"behavior": "allow", "updatedInput": input}),
                ))
                .await?;
            }
            Control::Other => self.send(&bridge::refusal(&id)).await?,
        }
        Ok(None)
    }

    fn delta(
        &mut self,
        client: &Client,
        request: &CompletionRequest<'_>,
        partial: &mut Partial,
        piece: Piece,
    ) -> Result<()> {
        let delta = match piece {
            Piece::TextDelta { text } => {
                partial.text.push_str(&text);
                Delta::Text(text)
            }
            Piece::ThinkingDelta { thinking } => {
                partial.reasoning.push_str(&thinking);
                Delta::Thinking(thinking)
            }
            Piece::Other => return Ok(()),
        };
        client.apply(request, &mut self.sizes, delta)
    }

    fn meter(&mut self, id: &str, usage: Option<super::super::wire::anthropic::Counted>) {
        let Some(usage) = usage.map(|usage| usage.usage()) else {
            return;
        };
        self.metered = true;
        match self.calls.iter_mut().find(|(seen, _)| seen == id) {
            Some((_, kept)) => *kept = usage,
            None => self.calls.push((id.to_string(), usage)),
        }
    }

    fn round_usage(&mut self) -> Usage {
        let mut total = Usage::default();
        for (_, usage) in self.calls.drain(..) {
            total.add(&usage);
        }
        self.spent.0 = self.spent.0.saturating_add(total.completion_tokens);
        self.spent.1 = self.spent.1.saturating_add(total.reasoning_tokens);
        total
    }

    fn collect(&mut self, content: Vec<Block>) {
        for block in content {
            match block {
                Block::Text { text } => self.text.push_str(&text),
                Block::Thinking { thinking } => self.reasoning.push_str(&thinking),
                Block::Other => {}
            }
        }
    }

    fn paused(&mut self) -> AssistantMessage {
        let tool_calls = self
            .pending
            .iter()
            .map(|call| ToolCall {
                id: call.id.clone(),
                kind: ToolKind::Function,
                function: ToolFunction {
                    checked: Default::default(),
                    name: call.name.clone(),
                    arguments: call.arguments.to_string(),
                },
                continuation: None,
            })
            .collect();
        AssistantMessage {
            content: taken(&mut self.text),
            reasoning_content: taken(&mut self.reasoning),
            tool_calls,
            usage: self.round_usage(),
            stop_reason: StopReason::ToolCalls,
            ..AssistantMessage::default()
        }
    }

    fn finish(&mut self, finished: Finished) -> Result<AssistantMessage> {
        if finished.is_error {
            return Err(self.failed(finished));
        }
        let content = taken(&mut self.text).or(finished.result.filter(|text| !text.is_empty()));
        let total = finished.usage.usage();
        let usage = if self.metered {
            let mut usage = self.round_usage();
            usage.completion_tokens += total.completion_tokens.saturating_sub(self.spent.0);
            usage.reasoning_tokens += total.reasoning_tokens.saturating_sub(self.spent.1);
            usage
        } else {
            total
        };
        Ok(AssistantMessage {
            content,
            reasoning_content: taken(&mut self.reasoning),
            usage,
            stop_reason: StopReason::Stop,
            ..AssistantMessage::default()
        })
    }

    fn failed(&self, finished: Finished) -> Error {
        let message = if finished.errors.is_empty() {
            finished
                .result
                .filter(|text| !text.is_empty())
                .unwrap_or(finished.subtype)
        } else {
            finished.errors.join("; ")
        };
        let status = finished.api_error_status;
        let kind = if self.rejected || status == Some(429) {
            ProviderFailureKind::RateLimited
        } else if matches!(status, Some(401 | 403)) {
            ProviderFailureKind::Authentication
        } else {
            ProviderFailureKind::Http
        };
        Error::provider(kind, format!("Claude Code: {message}"))
    }
}

fn taken(text: &mut String) -> Option<String> {
    let text = std::mem::take(text);
    (!text.is_empty()).then_some(text)
}
