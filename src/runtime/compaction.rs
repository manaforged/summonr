use super::turn::WaitError;
use super::*;
use crate::workflow;

pub(super) fn force_compact(history: &[Message]) -> Vec<(usize, Message)> {
    let live: Vec<usize> = history
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            message.role == Role::Tool && !message.content.starts_with("[cleared")
        })
        .map(|(index, _)| index)
        .collect();
    let target = live
        .iter()
        .map(|index| history[*index].content.len())
        .sum::<usize>()
        .div_ceil(2);
    let keep = live.len().saturating_sub(KEEP_TAIL).max(1).min(live.len());
    let mut reclaimed = 0;
    let mut edits = Vec::new();
    for index in live.into_iter().take(keep) {
        let message = &history[index];
        reclaimed += message.content.len();
        let call = message.tool_call_id.clone().unwrap_or_default();
        let cleared = format!("[cleared {} chars]", message.content.len());
        edits.push((index, Message::tool(call, cleared)));
        if reclaimed >= target {
            break;
        }
    }
    edits
}

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

pub(super) const CHARS_PER_TOKEN: usize = 4;
const KEEP_TAIL: usize = 2;
const COMPACT_PERCENT: usize = 80;
const RESERVE_TOKENS: u32 = 16_384;
const TAIL_TOKENS: u32 = 24_000;
const DEFAULT_COMPACT_CHARS: usize = 400_000;

const SUMMARY_ASK: &str = "Do not call tools. Summarize this session so it can continue without the raw history. \
    Exactly these sections, terse bullets: Goal / Constraints / Progress / Key decisions / \
    Failed approaches — what was tried and ruled out, so it is not retried / \
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

fn text_only(message: &Message) -> Message {
    match message.role {
        Role::Assistant if !message.tool_calls.is_empty() => {
            let mut lines = Vec::with_capacity(message.tool_calls.len() + 1);
            if !message.content.is_empty() {
                lines.push(message.content.clone());
            }
            lines.extend(message.tool_calls.iter().map(|call| {
                format!(
                    "[called {} {}]",
                    call.function.name, call.function.arguments
                )
            }));
            Message {
                content: lines.join("\n"),
                tool_calls: Vec::new(),
                continuation: None,
                ..message.clone()
            }
        }
        Role::Tool => {
            let label = if message.failed {
                "tool error"
            } else {
                "tool result"
            };
            Message {
                role: Role::User,
                content: format!("[{label}]\n{}", message.content),
                tool_call_id: None,
                failed: false,
                ..message.clone()
            }
        }
        Role::System | Role::User | Role::Assistant => message.clone(),
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
        self.session.rewrite(edits).map_err(super::session_err)?;
        Ok(())
    }

    async fn summarize(
        &mut self,
        system: &Message,
        slice: &[Message],
    ) -> Result<Option<AssistantMessage>> {
        let mut request_messages = Vec::with_capacity(slice.len() + 2);
        request_messages.push(system.clone());
        request_messages.extend(slice.iter().map(text_only));
        request_messages.push(Message::user(SUMMARY_ASK.to_string()));
        let request = self.request(workflow::Role::Summary, &request_messages, &[], false);
        match self
            .complete_once(workflow::Role::Summary, &request)
            .await?
        {
            Ok(message)
                if message.tool_calls.is_empty()
                    && message
                        .content
                        .as_ref()
                        .is_some_and(|text| !text.trim().is_empty()) =>
            {
                Ok(Some(message))
            }
            Ok(_) | Err(WaitError::Provider(_)) => Ok(None),
            Err(wait_error) => Err(wait_error.error()),
        }
    }

    pub(super) async fn auto_compact(
        &mut self,
        messages: &mut Vec<Message>,
        tool_bytes: usize,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if self.summary_wait > 0 {
            return Ok(());
        }
        let keep = self.provider.keeps_reasoning();
        let chars: usize = messages.iter().map(|m| message_chars(m, keep)).sum();
        let estimated_tokens = self.estimated_prompt_tokens(chars, tool_bytes);
        let window = match (self.context_window, self.compact_tokens) {
            (Some(window), Some(cap)) => Some(window.min(cap)),
            (window, cap) => window.or(cap),
        };
        let pressure = window.map_or(chars >= DEFAULT_COMPACT_CHARS, |window| {
            under_pressure(estimated_tokens, window)
        });
        if !pressure || messages.len() < 8 {
            return Ok(());
        }
        let tail_chars = window.map_or(DEFAULT_COMPACT_CHARS / 4, |window| {
            let tail_tokens = window
                .saturating_sub(RESERVE_TOKENS.min(window / 4))
                .div_ceil(4)
                .clamp(1, TAIL_TOKENS);
            chars
                .saturating_mul(tail_tokens as usize)
                .checked_div(estimated_tokens.max(1))
                .unwrap_or(chars / 4)
                .max(1)
        });
        let Some(keep_from) = compact_boundary(messages, tail_chars, keep, self.session.opener())
        else {
            return Ok(());
        };
        let Some(summary) = self
            .summarize(&messages[0], &messages[1..keep_from])
            .await?
        else {
            self.summary_wait = crate::limits::SUMMARY_RETRY_ROUNDS;
            self.push_event(
                events,
                emit,
                Event::Note {
                    text: format!(
                        "auto-compaction failed — keeping the live context and retrying \
                         after {} rounds",
                        crate::limits::SUMMARY_RETRY_ROUNDS
                    ),
                },
            )?;
            return Ok(());
        };
        self.count_usage(&summary, usage);
        self.emit_records(events, emit)?;
        if let Some(reason) = self.round_abort() {
            return Err(Error::Config(reason));
        }
        let summary = summary.content.unwrap_or_default();
        self.session
            .compact(&summary, &messages[keep_from..])
            .map_err(super::session_err)?;
        self.tools.forget_reads();
        messages.truncate(1);
        messages.extend(self.session.history().iter().cloned());
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "context auto-compacted — earlier turns summarized".to_string(),
            },
        )?;
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
        let Some(reply) = self
            .summarize(&messages[0], &messages[1..keep_from])
            .await?
        else {
            return Err(Error::Config(
                "the compaction summary failed; the context is unchanged".to_string(),
            ));
        };
        self.count_usage(&reply, &mut Usage::default());
        self.emit_records(&mut Vec::new(), &mut emit)?;
        if let Some(reason) = self.round_abort() {
            return Err(Error::Config(reason));
        }
        let summary = reply.content.unwrap_or_default();
        self.session
            .compact(&summary, &messages[keep_from..])
            .map_err(super::session_err)?;
        self.spent_tokens = self.session.spent_tokens();
        self.tools.forget_reads();
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
