use super::*;

pub(in crate::runtime) fn force_compact(history: &[Message]) -> Vec<(usize, Message)> {
    let live = live_outputs(history, history.len());
    let target = live
        .iter()
        .map(|index| history[*index].content.len())
        .sum::<usize>()
        .div_ceil(2);
    let keep = live
        .len()
        .saturating_sub(crate::limits::limits().compact_keep_tail)
        .max(1)
        .min(live.len());
    clear_outputs(history, &live[..keep], target).0
}

pub(super) fn live_outputs(history: &[Message], before: usize) -> Vec<usize> {
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

pub(super) fn elide_target(chars: usize, estimated_tokens: usize, window: Option<u32>) -> usize {
    let limits = crate::limits::limits();
    let from_chars =
        limits.compact_chars.saturating_mul(limits.elide_percent) / limits.compact_percent.max(1);
    window.map_or(from_chars, |window| {
        chars.saturating_mul(window as usize * limits.elide_percent / 100) / estimated_tokens.max(1)
    })
}

impl<P: Provider> Loop<P> {
    pub(super) fn clear_old_outputs(
        &mut self,
        messages: &mut [Message],
        chars: usize,
        estimated_tokens: usize,
        window: u32,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<bool> {
        let limits = crate::limits::limits();
        let trigger = (window as usize * limits.clear_percent / 100).min(limits.clear_tokens);
        if estimated_tokens < trigger || messages.len() < 2 {
            return Ok(false);
        }
        let live = live_outputs(&messages[1..], messages.len() - 1);
        let old = &live[..live.len().saturating_sub(limits.clear_keep)];
        let (edits, reclaimed) = clear_outputs(&messages[1..], old, usize::MAX);
        if reclaimed.saturating_mul(estimated_tokens) / chars.max(1)
            < trigger * limits.clear_at_least_percent / 100
        {
            return Ok(false);
        }
        let cleared = edits.len();
        self.rewrite(messages, edits)?;
        self.tools.forget_reads();
        self.push_event(
            events,
            emit,
            Event::Note {
                text: self.pressure_note(
                    "compacted",
                    &format!("at {trigger} tokens"),
                    &format!("cleared {cleared} old tool outputs"),
                ),
            },
        )?;
        Ok(true)
    }

    pub(super) fn elide_stale(
        &mut self,
        messages: &mut [Message],
        size: &Size,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<bool> {
        let target_chars = elide_target(size.chars, size.estimated_tokens, size.window);
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
                text: self.pressure_note(
                    "compacted",
                    &ceiling_label(size.window, size.chars),
                    &format!("cleared {cleared} old tool outputs"),
                ),
            },
        )?;
        Ok(true)
    }
}
