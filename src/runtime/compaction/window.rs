use super::*;

fn under_pressure(estimated_tokens: usize, window: u32) -> bool {
    estimated_tokens.saturating_mul(100)
        >= (window as usize).saturating_mul(crate::limits::limits().compact_percent)
}

pub(super) fn fired_ceiling(
    chars: usize,
    estimated_tokens: usize,
    known: Option<u32>,
    compact_tokens: Option<u32>,
) -> Option<Option<u32>> {
    let token_ceiling = match (known, compact_tokens) {
        (Some(window), Some(cap)) => Some(window.min(cap)),
        (Some(window), None) => Some(window),
        (None, Some(cap)) => Some(cap),
        (None, None) => None,
    };
    let token_trip = token_ceiling.is_some_and(|ceiling| under_pressure(estimated_tokens, ceiling));
    let char_trip = known.is_none() && chars >= crate::limits::limits().compact_chars;
    if token_trip {
        Some(token_ceiling)
    } else if char_trip {
        Some(None)
    } else {
        None
    }
}

pub(super) fn ceiling_label(window: Option<u32>, chars: usize) -> String {
    if let Some(ceiling) = window {
        format!(
            "at {}% of {ceiling}",
            crate::limits::limits().compact_percent
        )
    } else if chars >= crate::limits::limits().compact_chars {
        format!(
            "prompt exceeded {} characters",
            crate::limits::limits().compact_chars
        )
    } else {
        "context limit".to_string()
    }
}

impl<P: Provider> Loop<P> {
    pub(in crate::runtime) fn receiver_window(&self, role: workflow::Role) -> Option<u32> {
        match self.selections.get(&role) {
            Some((_, _, window)) => *window,
            None => self.context_window,
        }
    }

    pub(super) fn known_window(&self) -> Option<u32> {
        let round = self.receiver_window(crate::workflow::Role::Implement);
        let summary = self
            .selections
            .get(&workflow::Role::Summary)
            .and_then(|(_, _, window)| *window);
        match (round, summary) {
            (Some(round), Some(summary)) => Some(round.min(summary)),
            (Some(round), None) => Some(round),
            (None, Some(summary)) => Some(summary),
            (None, None) => None,
        }
    }

    pub(crate) fn pressure_window(&self) -> Option<u32> {
        match (self.known_window(), self.compact_tokens) {
            (Some(window), Some(cap)) => Some(window.min(cap)),
            (Some(window), None) => Some(window),
            (None, Some(cap)) => Some(cap),
            (None, None) => None,
        }
    }
}
