use super::turn::WaitError;
use super::*;
use crate::workflow;

mod clear;
mod window;

pub(super) use clear::force_compact;
use window::fired_ceiling;

pub(super) fn repair_arguments(history: &[Message]) -> Vec<(usize, Message)> {
    let Some(index) = history.iter().rposition(|m| m.role == Role::Assistant) else {
        return Vec::new();
    };
    let mut message = history[index].clone();
    for call in &mut message.tool_calls {
        if serde_json::from_str::<serde_json::Value>(&call.function.arguments).is_err() {
            call.function.arguments = "{}".to_string();
        }
    }
    vec![(index, message)]
}

struct Size {
    chars: usize,
    estimated_tokens: usize,
    window: Option<u32>,
    keep_from: usize,
}

pub(super) const CHARS_PER_TOKEN: usize = 4;

const SUMMARY_ASK: &str = "Do not call tools. Summarize this session so it can continue without the raw history. \
    Exactly these sections, terse bullets: Goal / Constraints / Progress / Key decisions / \
    Failed approaches (what was tried and ruled out, so it is not retried) / \
    Next steps / Files read / Files modified.";

pub(super) fn estimate(chars: usize, sample: Option<(usize, u32)>) -> usize {
    match sample {
        Some((sample_chars, tokens)) if sample_chars > 0 && tokens > 0 => {
            chars.saturating_mul(tokens as usize).div_ceil(sample_chars)
        }
        _ => chars.div_ceil(CHARS_PER_TOKEN),
    }
}

impl<P: Provider> Loop<P> {
    pub(super) fn rewrite(
        &mut self,
        messages: &mut [Message],
        edits: Vec<(usize, Message)>,
    ) -> Result<()> {
        if edits.is_empty() {
            return Ok(());
        }
        for (index, message) in &edits {
            if let Some(slot) = messages.get_mut(index + 1) {
                slot.clone_from(message);
            }
        }
        self.session.rewrite(edits)?;
        Ok(())
    }

    async fn summarize(
        &mut self,
        system: &Message,
        slice: &[Message],
    ) -> Result<Option<AssistantMessage>> {
        let mut request_messages = Vec::with_capacity(slice.len() + 2);
        request_messages.push(system.clone());
        request_messages.extend_from_slice(slice);
        request_messages.push(Message::user(SUMMARY_ASK.to_string()));
        let advertised = self.advertised_tools();
        match self
            .side_call(
                workflow::Role::Summary,
                &request_messages,
                Some(&advertised),
            )
            .await?
            .map_err(|missed| missed.error)
        {
            Ok(reply)
                if reply
                    .message
                    .content
                    .as_ref()
                    .is_some_and(|text| !text.trim().is_empty()) =>
            {
                Ok(Some(reply.message))
            }
            Ok(_) => Ok(None),
            Err(WaitError::Provider(error)) => {
                if let Some(cap) = crate::provider::input_cap(&error.to_string()) {
                    self.learn_role_window(workflow::Role::Summary, cap);
                }
                Ok(None)
            }
            Err(wait_error) => Err(wait_error.error()),
        }
    }

    async fn summarize_until(
        &mut self,
        messages: &[Message],
        keep_from: usize,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<String>> {
        let Some(reply) = self
            .summarize(&messages[0], &messages[1..keep_from])
            .await?
        else {
            return Ok(None);
        };
        usage.add(&reply.usage);
        self.emit_records(events, emit)?;
        if let Some(reason) = self.round_abort() {
            return Err(Error::Config(reason));
        }
        let summary = reply.content.unwrap_or_default();
        self.session.compact(&summary, &messages[keep_from..])?;
        self.tools.forget_reads();
        Ok(Some(summary))
    }

    pub(super) async fn auto_compact(
        &mut self,
        messages: &mut Vec<Message>,
        tool_bytes: usize,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<bool> {
        if self.summary_wait > 0 {
            return Ok(false);
        }
        let parts = self.measure_parts(messages, tool_bytes);
        self.prompt_parts = parts;
        let chars = parts.0 + parts.2 + parts.3;
        let estimated_tokens = self.estimated_prompt_tokens(chars, tool_bytes);
        let known = self.known_window();
        if let Some(window) = known
            && self.clear_old_outputs(messages, chars, estimated_tokens, window)?
        {
            return Ok(true);
        }
        let Some(window) = fired_ceiling(chars, estimated_tokens, known, self.compact_tokens)
        else {
            return Ok(false);
        };
        let keep = self.provider.keeps_reasoning();
        let compact_chars = crate::limits::limits().compact_chars;
        if messages.len() < 8 {
            return Ok(false);
        }
        let tail_chars = tail_chars(window, chars, estimated_tokens, compact_chars);
        let Some(keep_from) = compact_boundary(messages, tail_chars, keep, self.session.opener())
        else {
            return Ok(false);
        };
        let size = Size {
            chars,
            estimated_tokens,
            window,
            keep_from,
        };
        if self.elide_stale(messages, &size)? {
            return Ok(true);
        }
        let Some(summary) = self
            .summarize_until(messages, keep_from, usage, events, emit)
            .await?
        else {
            self.summary_wait = crate::limits::limits().summary_retry_rounds;
            self.push_event(
                events,
                emit,
                Event::Note {
                    text: format!(
                        "auto-compaction failed; keeping the live context and retrying \
                         after {} rounds",
                        crate::limits::limits().summary_retry_rounds
                    ),
                },
            )?;
            return Ok(false);
        };
        messages.truncate(1);
        messages.extend(self.session.history().iter().cloned());
        self.compact_hook(&summary, events, emit).await?;
        Ok(true)
    }

    fn measure_parts(
        &self,
        messages: &[Message],
        tool_bytes: usize,
    ) -> (usize, usize, usize, usize) {
        let keep = self.provider.keeps_reasoning();
        let mut system = 0;
        let mut tools = 0;
        let mut other = 0;
        for message in messages {
            let chars = message_chars(message, keep);
            match message.role {
                Role::System => system += chars,
                Role::Tool => tools += chars,
                _ => other += chars,
            }
        }
        (system, tool_bytes, tools, other)
    }

    pub(crate) fn prompt_parts(&self) -> (usize, usize, usize, usize) {
        self.prompt_parts
    }

    async fn compact_hook(
        &mut self,
        summary: &str,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let payload = serde_json::json!({"event": "compact", "summary": summary});
        if let Some(note) = self
            .hook(crate::hooks::HookEvent::Compact, None, payload)
            .await
            .note()
        {
            self.push_event(
                events,
                emit,
                Event::Note {
                    text: format!("compact hook: {note}"),
                },
            )?;
        }
        Ok(())
    }

    pub async fn compact(&mut self) -> Result<bool> {
        self.compact_with(|_| {}).await
    }

    pub(crate) async fn compact_with(&mut self, mut emit: impl FnMut(&Event)) -> Result<bool> {
        let mut messages = Vec::with_capacity(self.session.history().len() + 1);
        messages.push(Message::system(self.system_prompt.clone()));
        messages.extend(self.session.history().iter().cloned());
        let Some(keep_from) = messages
            .iter()
            .rposition(|message| message.role == Role::User)
            .filter(|index| *index > 1)
        else {
            return Ok(false);
        };
        if self
            .summarize_until(
                &messages,
                keep_from,
                &mut Usage::default(),
                &mut Vec::new(),
                &mut emit,
            )
            .await?
            .is_none()
        {
            return Err(Error::Config(
                "the compaction summary failed; the context is unchanged".to_string(),
            ));
        }
        Ok(true)
    }
}

pub(super) fn message_chars(message: &Message, reasoning_on_wire: bool) -> usize {
    message.content.len()
        + if reasoning_on_wire {
            message.reasoning.as_ref().map_or(0, String::len)
        } else {
            0
        }
        + message
            .tool_calls
            .iter()
            .map(|call| call.function.arguments.len() + call.function.name.len())
            .sum::<usize>()
}

fn tail_chars(
    window: Option<u32>,
    chars: usize,
    estimated_tokens: usize,
    compact_chars: usize,
) -> usize {
    window.map_or(compact_chars / 4, |window| {
        let tail_tokens = window
            .saturating_sub(
                crate::limits::limits()
                    .compact_reserve_tokens
                    .min(window / 4),
            )
            .div_ceil(4)
            .clamp(1, crate::limits::limits().compact_tail_tokens);
        chars
            .saturating_mul(tail_tokens as usize)
            .checked_div(estimated_tokens.max(1))
            .unwrap_or(chars / 4)
            .max(1)
    })
}

fn tail_start(messages: &[Message], target_chars: usize, reasoning_on_wire: bool) -> usize {
    let mut chars = 0;
    let mut start = messages.len();
    while start > 1 && chars < target_chars {
        start -= 1;
        chars += message_chars(&messages[start], reasoning_on_wire);
    }
    start
}

fn compact_boundary(
    messages: &[Message],
    target_chars: usize,
    reasoning_on_wire: bool,
    opener: Option<usize>,
) -> Option<usize> {
    let floor = opener.map_or(1, |index| (index + 2).min(messages.len()));
    let mut start = tail_start(messages, target_chars, reasoning_on_wire).max(floor);
    while start > floor && messages.get(start).is_some_and(|m| m.role == Role::Tool) {
        start -= 1;
    }
    (start > 1).then_some(start)
}

#[cfg(test)]
#[path = "compaction_tests.rs"]
mod tests;
