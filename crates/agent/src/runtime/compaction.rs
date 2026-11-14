use super::hooks::note_event;
use super::turn::WaitError;
use super::*;
use crate::hooks::{Compaction, HookEvent};
use crate::workflow;

mod clear;
mod hook;
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
    pub(super) fn rewrite(&mut self, edits: Vec<(usize, Message)>) -> Result<()> {
        if edits.is_empty() {
            return Ok(());
        }
        self.session.rewrite(edits)?;
        Ok(())
    }

    async fn summarize(
        &mut self,
        system: &str,
        slice: &[Message],
        instructions: &str,
    ) -> Result<Option<AssistantMessage>> {
        let mut request_messages = Vec::with_capacity(slice.len() + 2);
        request_messages.push(Message::system(system.to_string()));
        request_messages.extend_from_slice(slice);
        request_messages.push(Message::user(crate::session::decorate(
            instructions,
            SUMMARY_ASK,
        )));
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
        keep_from: usize,
        hooked: (Compaction, &str),
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<String>> {
        let (trigger, instructions) = hooked;
        let system = self.system_prompt.clone();
        let slice = self.session.history()[..keep_from].to_vec();
        let Some(reply) = self.summarize(&system, &slice, instructions).await? else {
            return Ok(None);
        };
        usage.add(&reply.usage);
        self.emit_records(events, emit)?;
        if let Some(reason) = self.round_abort() {
            return Err(Error::Config(reason));
        }
        let mut summary = reply.content.unwrap_or_default();
        if let Some(context) = self.restarted(events, emit).await? {
            summary = crate::session::decorate(&context, &summary);
        }
        let tail = self.session.history()[keep_from..].to_vec();
        self.session.compact(&summary, &tail)?;
        self.tools.forget_reads();
        self.outputs_cleared = false;
        self.compacted(&summary, trigger, events, emit).await?;
        Ok(Some(summary))
    }

    pub(super) async fn auto_compact(
        &mut self,
        tool_bytes: usize,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<bool> {
        if self.summary_wait > 0 {
            return Ok(false);
        }
        let parts = self.measure_parts(self.session.history(), tool_bytes);
        self.prompt_parts = parts;
        let chars = parts.0 + parts.2 + parts.3;
        let estimated_tokens = self.estimated_prompt_tokens(chars, tool_bytes);
        let known = self.known_window();
        if let Some(window) = known
            && !self.outputs_cleared
            && self.clear_old_outputs(chars, estimated_tokens, window)?
        {
            self.outputs_cleared = true;
            return Ok(true);
        }
        let Some(window) = fired_ceiling(chars, estimated_tokens, known, self.compact_tokens)
        else {
            return Ok(false);
        };
        let keep = self.provider.keeps_reasoning();
        let compact_chars = crate::limits::limits().compact_chars;
        let history = self.session.history();
        if history.len() < 7 {
            return Ok(false);
        }
        let tail_chars = tail_chars(window, chars, estimated_tokens, compact_chars);
        let Some(keep_from) =
            compact_history_boundary(history, tail_chars, keep, self.session.opener())
        else {
            return Ok(false);
        };
        let size = Size {
            chars,
            estimated_tokens,
            window,
            keep_from,
        };
        if self.elide_stale(&size)? {
            return Ok(true);
        }
        let instructions = match self.pre_compact(Compaction::Auto, events, emit).await? {
            Ok(instructions) => instructions,
            Err(reason) => {
                self.summary_wait = crate::limits::limits().summary_retry_rounds;
                let text = format!("skipped auto-compaction: {reason}");
                self.push_event(events, emit, note_event(HookEvent::PreCompact, &text))?;
                return Ok(false);
            }
        };
        let hooked = (Compaction::Auto, instructions.as_str());
        if self
            .summarize_until(keep_from, hooked, usage, events, emit)
            .await?
            .is_none()
        {
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
        }
        Ok(true)
    }

    fn measure_parts(
        &self,
        history: &[Message],
        tool_bytes: usize,
    ) -> (usize, usize, usize, usize) {
        let keep = self.provider.keeps_reasoning();
        let mut system = self.system_prompt.len();
        let mut tools = 0;
        let mut other = 0;
        for message in history {
            let chars = message_chars(message, keep);
            match message.role {
                Role::System => system += chars,
                Role::Tool => tools += chars,
                Role::User | Role::Assistant => other += chars,
            }
        }
        (system, tool_bytes, tools, other)
    }

    pub(crate) fn prompt_parts(&self) -> (usize, usize, usize, usize) {
        self.prompt_parts
    }

    pub(super) fn estimated_prompt_tokens(&self, message_chars: usize, tool_bytes: usize) -> usize {
        estimate(message_chars + tool_bytes, self.context_sample)
    }

    pub async fn compact(&mut self) -> Result<bool> {
        self.compact_with(|_| {}).await
    }

    pub(crate) async fn compact_with(&mut self, mut emit: impl FnMut(&Event)) -> Result<bool> {
        let Some(keep_from) = self
            .session
            .history()
            .iter()
            .rposition(|message| message.role == Role::User)
            .filter(|index| *index > 0)
        else {
            return Ok(false);
        };
        let mut events = Vec::new();
        let instructions = self
            .pre_compact(Compaction::Manual, &mut events, &mut emit)
            .await?
            .map_err(|reason| {
                Error::Denied(format!("the pre_compact hook blocked compaction: {reason}"))
            })?;
        let hooked = (Compaction::Manual, instructions.as_str());
        let mut usage = Usage::default();
        if self
            .summarize_until(keep_from, hooked, &mut usage, &mut events, &mut emit)
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

fn tail_start(
    messages: &[Message],
    lead: usize,
    target_chars: usize,
    reasoning_on_wire: bool,
) -> usize {
    let mut chars = 0;
    let mut start = messages.len();
    while start > lead && chars < target_chars {
        start -= 1;
        chars += message_chars(&messages[start], reasoning_on_wire);
    }
    start
}

fn compact_history_boundary(
    history: &[Message],
    target_chars: usize,
    reasoning_on_wire: bool,
    opener: Option<usize>,
) -> Option<usize> {
    boundary_in(history, 0, target_chars, reasoning_on_wire, opener)
}

#[cfg(test)]
fn compact_boundary(
    messages: &[Message],
    target_chars: usize,
    reasoning_on_wire: bool,
    opener: Option<usize>,
) -> Option<usize> {
    boundary_in(messages, 1, target_chars, reasoning_on_wire, opener)
}

fn boundary_in(
    messages: &[Message],
    lead: usize,
    target_chars: usize,
    reasoning_on_wire: bool,
    opener: Option<usize>,
) -> Option<usize> {
    let summary = messages
        .iter()
        .rposition(|message| message.role == Role::System)
        .map_or(lead, |index| index + 1);
    let floor = opener
        .map_or(lead, |index| (index + 1 + lead).min(messages.len()))
        .max(summary);
    let mut start = tail_start(messages, lead, target_chars, reasoning_on_wire).max(floor);
    while start > floor && messages.get(start).is_some_and(|m| m.role == Role::Tool) {
        start -= 1;
    }
    (start > lead).then_some(start)
}

#[cfg(test)]
#[path = "compaction_tests.rs"]
mod tests;
