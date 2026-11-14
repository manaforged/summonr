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
        let cleared = format!(
            "[cleared {} chars; run the call again before relying on it]",
            message.content.len()
        );
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
        chars: usize,
        estimated_tokens: usize,
        window: u32,
    ) -> Result<bool> {
        let limits = crate::limits::limits();
        let trigger = (window as usize * limits.clear_percent / 100).min(limits.clear_tokens);
        let history = self.session.history();
        if estimated_tokens < trigger || history.is_empty() {
            return Ok(false);
        }
        let live = live_outputs(history, history.len());
        let old = &live[..live.len().saturating_sub(limits.clear_keep)];
        let (edits, reclaimed) = clear_outputs(history, old, usize::MAX);
        if reclaimed.saturating_mul(estimated_tokens) / chars.max(1)
            < trigger * limits.clear_at_least_percent / 100
        {
            return Ok(false);
        }
        self.rewrite(edits)?;
        self.tools.forget_reads();
        Ok(true)
    }

    pub(super) fn elide_stale(&mut self, size: &Size) -> Result<bool> {
        let target_chars = elide_target(size.chars, size.estimated_tokens, size.window);
        let needed = size.chars.saturating_sub(target_chars);
        let stale = live_outputs(self.session.history(), size.keep_from);
        let (edits, reclaimed) = clear_outputs(self.session.history(), &stale, needed);
        if needed == 0 || reclaimed < needed {
            return Ok(false);
        }
        self.rewrite(edits)?;
        self.tools.forget_reads();
        Ok(true)
    }
}
