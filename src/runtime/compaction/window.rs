use super::*;

fn under_pressure(estimated_tokens: usize, window: u32) -> bool {
    estimated_tokens.saturating_mul(100)
        >= (window as usize).saturating_mul(crate::limits::limits().compact_percent)
}

fn smaller(left: Option<u32>, right: Option<u32>) -> Option<u32> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    }
}

pub(super) fn fired_ceiling(
    chars: usize,
    estimated_tokens: usize,
    known: Option<u32>,
    compact_tokens: Option<u32>,
) -> Option<Option<u32>> {
    let token_ceiling = smaller(known, compact_tokens);
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

impl<P: Provider> Loop<P> {
    pub(in crate::runtime) fn receiver_window(&self, role: workflow::Role) -> Option<u32> {
        match self.selections.get(&role) {
            Some((_, _, window)) => *window,
            None => self.context_window,
        }
    }

    pub(super) fn known_window(&self) -> Option<u32> {
        let summary = self
            .selections
            .get(&workflow::Role::Summary)
            .and_then(|(_, _, window)| *window);
        smaller(self.receiver_window(workflow::Role::Implement), summary)
    }

    pub(crate) fn pressure_window(&self) -> Option<u32> {
        smaller(self.known_window(), self.compact_tokens)
    }
}
