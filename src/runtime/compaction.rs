use super::*;
use crate::workflow;

pub(super) fn force_compact(messages: &mut [Message]) -> bool {
    let target = messages
        .iter()
        .filter(|message| message.role == Role::Tool)
        .map(|message| message.content.len())
        .sum::<usize>()
        .div_ceil(2);
    let mut reclaimed = 0;
    let mut changed = false;
    for message in messages.iter_mut().rev() {
        if message.role == Role::Tool && !message.content.starts_with("[cleared") {
            reclaimed += message.content.len();
            message.content = format!("[cleared {} chars]", message.content.len());
            changed = true;
            if reclaimed >= target {
                break;
            }
        }
    }
    changed
}

pub(super) const CHARS_PER_TOKEN: usize = 4;
const COMPACT_PERCENT: usize = 80;
const RESERVE_TOKENS: u32 = 16_384;
const DEFAULT_COMPACT_CHARS: usize = 400_000;

const SUMMARY_ASK: &str = "Do not call tools. Summarize this session so it can continue without the raw history. \
    Exactly these sections, terse bullets: Goal / Constraints / Progress / Key decisions / \
    Next steps / Files read / Files modified.";

pub(super) fn estimate(chars: usize, sample: Option<(usize, u32)>) -> usize {
    match sample {
        Some((sample_chars, tokens)) if chars >= sample_chars && tokens > 0 => {
            tokens as usize + chars.saturating_sub(sample_chars).div_ceil(CHARS_PER_TOKEN)
        }
        _ => chars.div_ceil(CHARS_PER_TOKEN),
    }
}

fn under_pressure(estimated_tokens: usize, window: u32) -> bool {
    estimated_tokens.saturating_mul(100) >= (window as usize).saturating_mul(COMPACT_PERCENT)
}

impl<P: Provider> Loop<P> {
    async fn summarize(
        &mut self,
        system: &Message,
        slice: &[Message],
        tools: &[crate::provider::ToolDefinition],
    ) -> Result<Option<(AssistantMessage, String)>> {
        let mut request_messages = Vec::with_capacity(slice.len() + 2);
        request_messages.push(system.clone());
        request_messages.extend(slice.iter().cloned());
        request_messages.push(Message::user(SUMMARY_ASK.to_string()));
        let request = CompletionRequest {
            model: self.seat(workflow::Role::Summary).1.to_string(),
            messages: request_messages,
            tools: tools.to_vec(),
            max_output_tokens: crate::runtime::DEFAULT_ROUND_OUTPUT_TOKENS,
            emit_deltas: false,
            effort: self.effort_for(workflow::Role::Summary),
        };
        let prefix_hash = request.prefix_hash();
        let durable = request
            .durable_bytes()
            .map_err(|error| Error::Config(format!("cannot encode provider request: {error}")))?;
        let attempt = self
            .session
            .start_summary(&request.model, super::exposure(&request, &durable))
            .map_err(super::session_err)?;
        let started = Instant::now();
        match self
            .seat(workflow::Role::Summary)
            .0
            .complete(&request)
            .await
        {
            Ok(message)
                if message.tool_calls.is_empty()
                    && message
                        .content
                        .as_ref()
                        .is_some_and(|text| !text.trim().is_empty()) =>
            {
                let meter = super::meter(&message.usage, super::elapsed_millis(started));
                let text = message.content.clone().unwrap_or_default();
                self.session
                    .finish_provider(attempt, &text, "", meter)
                    .map_err(super::session_err)?;
                Ok(Some((message, prefix_hash)))
            }
            Ok(_) | Err(_) => {
                self.session
                    .fail_provider(
                        attempt,
                        "compaction summary failed",
                        super::elapsed_millis(started),
                    )
                    .map_err(super::session_err)?;
                self.spent_tokens = self.session.replay().spent_tokens;
                Ok(None)
            }
        }
    }

    pub(super) async fn auto_compact(
        &mut self,
        messages: &mut Vec<Message>,
        tools: &[crate::provider::ToolDefinition],
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let chars: usize = messages.iter().map(message_chars).sum();
        let estimated_tokens = self.estimated_prompt_tokens(messages, tools);
        let pressure = self
            .context_window
            .map_or(chars >= DEFAULT_COMPACT_CHARS, |window| {
                under_pressure(estimated_tokens, window)
            });
        if !pressure || messages.len() < 8 {
            return Ok(());
        }
        let tail_chars = self
            .context_window
            .map_or(DEFAULT_COMPACT_CHARS / 4, |window| {
                let tail_tokens = window.saturating_sub(RESERVE_TOKENS).div_ceil(2).max(1);
                chars
                    .saturating_mul(tail_tokens as usize)
                    .checked_div(estimated_tokens.max(1))
                    .unwrap_or(chars / 4)
                    .max(1)
            });
        let mut keep_from = tail_start(messages, tail_chars);
        while keep_from > 1 && messages[keep_from].role != Role::User {
            keep_from -= 1;
        }
        if keep_from <= 1 {
            return Ok(());
        }
        let Some((summary, prefix_hash)) = self
            .summarize(&messages[0], &messages[1..keep_from], tools)
            .await?
        else {
            self.push_event(
                events,
                emit,
                Event::Note {
                    text: "auto-compaction failed — keeping the live context".to_string(),
                },
            )?;
            return Ok(());
        };
        self.record_usage(
            &summary,
            prefix_hash,
            workflow::Role::Summary,
            usage,
            events,
            emit,
        )?;
        let summary = summary.content.unwrap_or_default();
        self.session
            .compact(&summary, &messages[keep_from..])
            .map_err(super::session_err)?;
        let replayed = self.session.replay();
        messages.truncate(1);
        messages.extend(replayed.history.iter().cloned());
        self.commit(messages);
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "context auto-compacted — earlier turns summarized".to_string(),
            },
        )?;
        Ok(())
    }

    pub async fn compact(&mut self) -> Result<bool> {
        let mut messages = Vec::with_capacity(self.history.len() + 1);
        messages.push(Message::system(self.system_prompt.clone()));
        messages.extend(self.history.iter().cloned());
        let Some(keep_from) = messages
            .iter()
            .rposition(|message| message.role == Role::User)
            .filter(|index| *index > 1)
        else {
            return Ok(false);
        };
        let mut request_messages = messages[..keep_from].to_vec();
        request_messages.push(Message::user(SUMMARY_ASK.to_string()));
        let request = CompletionRequest {
            model: self.seat(workflow::Role::Summary).1.to_string(),
            messages: request_messages,
            tools: Vec::new(),
            max_output_tokens: crate::runtime::DEFAULT_ROUND_OUTPUT_TOKENS,
            emit_deltas: false,
            effort: self.effort_for(workflow::Role::Summary),
        };
        let reply = self
            .seat(workflow::Role::Summary)
            .0
            .complete(&request)
            .await?;
        if !reply.tool_calls.is_empty() {
            return Err(Error::Config(
                "the compaction summary tried to call a tool; the context is unchanged".to_string(),
            ));
        }
        let summary = reply
            .content
            .filter(|text| !text.trim().is_empty())
            .ok_or_else(|| Error::Config("the compaction summary was empty".to_string()))?;
        let model = request.model.clone();
        self.session
            .charge(super::meter(&reply.usage, 0), "manual compaction", &model)
            .map_err(super::session_err)?;
        self.session
            .compact(&summary, &messages[keep_from..])
            .map_err(super::session_err)?;
        self.spent_tokens = self.session.replay().spent_tokens;
        self.commit(&messages);
        Ok(true)
    }
}

pub(super) fn message_chars(message: &Message) -> usize {
    message.content.len()
        + message.reasoning.as_ref().map_or(0, String::len)
        + message
            .tool_calls
            .iter()
            .map(|call| call.function.arguments.len() + call.function.name.len())
            .sum::<usize>()
}

fn tail_start(messages: &[Message], target_chars: usize) -> usize {
    let mut chars = 0;
    let mut start = messages.len();
    while start > 1 && chars < target_chars {
        start -= 1;
        chars += message_chars(&messages[start]);
    }
    start
}

#[cfg(test)]
#[path = "compaction_tests.rs"]
mod tests;
