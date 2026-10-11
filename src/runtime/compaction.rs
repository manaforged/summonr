use super::turn::WaitError;
use super::*;
use crate::workflow;

pub(super) fn force_compact(history: &[Message]) -> Vec<(usize, Message)> {
    let live = live_outputs(history, history.len());
    let target = live
        .iter()
        .map(|index| history[*index].content.len())
        .sum::<usize>()
        .div_ceil(2);
    let keep = live.len().saturating_sub(KEEP_TAIL).max(1).min(live.len());
    clear_outputs(history, &live[..keep], target).0
}

fn live_outputs(history: &[Message], before: usize) -> Vec<usize> {
    history[..before.min(history.len())]
        .iter()
        .enumerate()
        .filter(|(_, message)| {
            message.role == Role::Tool && !message.content.starts_with("[cleared")
        })
        .map(|(index, _)| index)
        .collect()
}

fn clear_outputs(
    history: &[Message],
    candidates: &[usize],
    target: usize,
) -> (Vec<(usize, Message)>, usize) {
    let mut reclaimed = 0;
    let mut edits = Vec::new();
    for &index in candidates {
        if reclaimed >= target {
            break;
        }
        let message = &history[index];
        let cleared = format!("[cleared {} chars]", message.content.len());
        if cleared.len() >= message.content.len() {
            continue;
        }
        reclaimed += message.content.len() - cleared.len();
        let call = message.tool_call_id.clone().unwrap_or_default();
        edits.push((index, Message::tool(call, cleared)));
    }
    (edits, reclaimed)
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

struct Size {
    chars: usize,
    estimated_tokens: usize,
    window: Option<u32>,
    keep_from: usize,
}

pub(super) const CHARS_PER_TOKEN: usize = 4;
const KEEP_TAIL: usize = 2;
const RESERVE_TOKENS: u32 = 16_384;
const TAIL_TOKENS: u32 = 24_000;

const SUMMARY_ASK: &str = "Do not call tools. Summarize this session so it can continue without the raw history. \
    Exactly these sections, terse bullets: Goal / Constraints / Progress / Key decisions / \
    Failed approaches (what was tried and ruled out, so it is not retried) / \
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
    estimated_tokens.saturating_mul(100)
        >= (window as usize).saturating_mul(crate::limits::limits().compact_percent)
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
        request_messages.extend_from_slice(slice);
        request_messages.push(Message::user(SUMMARY_ASK.to_string()));
        let advertised = self.advertised_tools();
        let request = self
            .request(
                workflow::Role::Summary,
                &request_messages,
                &advertised.tools,
                false,
            )
            .with_tools_json(&advertised.json);
        match self
            .complete_once(workflow::Role::Summary, &request)
            .await?
        {
            Ok(message)
                if message
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
    ) -> Result<bool> {
        if self.summary_wait > 0 {
            return Ok(false);
        }
        let keep = self.provider.keeps_reasoning();
        let chars: usize = messages.iter().map(|m| message_chars(m, keep)).sum();
        let estimated_tokens = self.estimated_prompt_tokens(chars, tool_bytes);
        let window = self.compact_window();
        let compact_chars = crate::limits::limits().compact_chars;
        let pressure = window.map_or(chars >= compact_chars, |window| {
            under_pressure(estimated_tokens, window)
        });
        if !pressure || messages.len() < 8 {
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
        if self.elide_stale(messages, &size, events, emit)? {
            return Ok(true);
        }
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "compacting the context…".to_string(),
            },
        )?;
        let Some(summary) = self
            .summarize(&messages[0], &messages[1..keep_from])
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
                text: "context auto-compacted: earlier turns summarized".to_string(),
            },
        )?;
        self.compact_hook(&summary, events, emit).await?;
        Ok(true)
    }

    fn compact_window(&self) -> Option<u32> {
        match (self.context_window, self.compact_tokens) {
            (Some(window), Some(cap)) => Some(window.min(cap)),
            (window, cap) => window.or(cap),
        }
    }

    fn elide_stale(
        &mut self,
        messages: &mut [Message],
        size: &Size,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<bool> {
        let limits = crate::limits::limits();
        let target_chars = size.window.map_or(
            limits.compact_chars.saturating_mul(limits.elide_percent)
                / limits.compact_percent.max(1),
            |window| {
                size.chars
                    .saturating_mul(window as usize * limits.elide_percent / 100)
                    / size.estimated_tokens.max(1)
            },
        );
        let needed = size.chars.saturating_sub(target_chars);
        let stale = live_outputs(&messages[1..], size.keep_from - 1);
        let (edits, reclaimed) = clear_outputs(&messages[1..], &stale, needed);
        if needed == 0 || reclaimed < needed {
            return Ok(false);
        }
        let cleared = edits.len();
        self.rewrite(messages, edits)?;
        self.tools.forget_reads();
        self.push_event(
            events,
            emit,
            Event::Note {
                text: format!("context compacted: cleared {cleared} old tool outputs"),
            },
        )?;
        Ok(true)
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

fn tail_chars(
    window: Option<u32>,
    chars: usize,
    estimated_tokens: usize,
    compact_chars: usize,
) -> usize {
    window.map_or(compact_chars / 4, |window| {
        let tail_tokens = window
            .saturating_sub(RESERVE_TOKENS.min(window / 4))
            .div_ceil(4)
            .clamp(1, TAIL_TOKENS);
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
