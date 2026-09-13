use std::collections::VecDeque;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(3);
const FLOOR: f64 = 0.25;

#[derive(Clone, Debug)]
pub(super) struct Speed {
    chars_per_token: f64,
    samples: VecDeque<(Instant, usize)>,
    first: Option<Instant>,
    last: Option<Instant>,
    chars: usize,
    round: Option<Instant>,
    rate: Option<f64>,
}

impl Default for Speed {
    fn default() -> Self {
        Self {
            chars_per_token: 4.0,
            samples: VecDeque::new(),
            first: None,
            last: None,
            chars: 0,
            round: None,
            rate: None,
        }
    }
}

impl Speed {
    pub(super) fn start(&mut self, now: Instant) {
        self.round = Some(now);
        self.clear();
    }

    pub(super) fn delta(&mut self, chars: usize, now: Instant) {
        if self.round.is_none() || chars == 0 {
            return;
        }
        self.first.get_or_insert(now);
        self.last = Some(now);
        self.chars += chars;
        self.samples.push_back((now, chars));
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > WINDOW)
        {
            self.samples.pop_front();
        }
    }

    pub(super) fn live(&self, now: Instant) -> Option<f64> {
        let first = self.first?;
        let chars: usize = self
            .samples
            .iter()
            .filter(|(at, _)| now.duration_since(*at) <= WINDOW)
            .map(|(_, chars)| *chars)
            .sum();
        if chars == 0 {
            return None;
        }
        let span = now
            .duration_since(first)
            .min(WINDOW)
            .as_secs_f64()
            .max(FLOOR);
        Some(chars as f64 / self.chars_per_token / span)
    }

    pub(super) fn finish(&mut self, tokens: u32, now: Instant) {
        let Some(round) = self.round else {
            return;
        };
        if tokens > 0 {
            let span = match (self.first, self.last) {
                (Some(first), Some(last)) => last.duration_since(first),
                _ => now.duration_since(round),
            }
            .as_secs_f64()
            .max(FLOOR);
            self.rate = Some(f64::from(tokens) / span);
            if self.chars > 0 {
                self.chars_per_token = (self.chars as f64 / f64::from(tokens)).clamp(1.0, 10.0);
            }
        }
        self.round = Some(now);
        self.clear();
    }

    pub(super) fn last(&self) -> Option<f64> {
        self.rate
    }

    pub(super) fn label(rate: f64) -> String {
        format!("{rate:.0} tok/s")
    }

    fn clear(&mut self) {
        self.samples.clear();
        self.first = None;
        self.last = None;
        self.chars = 0;
    }
}
