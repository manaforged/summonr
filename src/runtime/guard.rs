use std::collections::HashMap;
use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const WINDOW: usize = 24;
const WARN: usize = 3;
const STOP: usize = 6;
const CYCLE_MAX_PERIOD: usize = 8;
const CYCLE_MIN_REPS: usize = 3;
const SIGNATURE_MIN_WORDS: usize = 3;

#[derive(Clone, Debug)]
pub(super) struct Settings {
    pub enabled: bool,
    pub signature_warn: usize,
    pub signature_stop: usize,
    pub cycles: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            signature_warn: 4,
            signature_stop: 8,
            cycles: true,
        }
    }
}

impl Settings {
    pub(super) fn from(config: &crate::config::Guard) -> Self {
        let defaults = Self::default();
        Self {
            enabled: config.enabled.unwrap_or(defaults.enabled),
            signature_warn: config.signature_warn.unwrap_or(defaults.signature_warn),
            signature_stop: config.signature_stop.unwrap_or(defaults.signature_stop),
            cycles: config.cycles.unwrap_or(defaults.cycles),
        }
    }
}

#[derive(Default)]
pub(super) struct Guard {
    seen: VecDeque<(u64, u64)>,
    stuck: Option<String>,
    failures: usize,
    strained: bool,
    rejected: bool,
    steered: bool,
    signatures: HashMap<u64, usize>,
    settings: Settings,
}

impl Guard {
    pub(super) fn configured(config: &crate::config::Guard) -> Self {
        Self {
            settings: Settings::from(config),
            ..Self::default()
        }
    }

    pub(super) fn reset(&mut self) {
        *self = Self {
            settings: self.settings.clone(),
            ..Self::default()
        };
    }

    pub(super) fn observe(
        &mut self,
        name: &str,
        arguments: &str,
        success: bool,
        output: String,
    ) -> String {
        if !self.settings.enabled {
            return output;
        }
        self.failures = if success { 0 } else { self.failures + 1 };
        if self.failures >= WARN {
            self.strained = true;
        }
        let normalized = normalize(&output);
        let key = (hash(&(name, arguments)), hash(&normalized));
        self.seen.push_back(key);
        if self.seen.len() > WINDOW {
            self.seen.pop_front();
        }
        let count = self.seen.iter().filter(|seen| **seen == key).count();
        if count >= STOP {
            self.stuck = Some(format!(
                "loop: `{name}` returned the same result {count} times in the last {} calls",
                self.seen.len()
            ));
        }
        if !success && normalized.split(' ').count() >= SIGNATURE_MIN_WORDS {
            let hits = {
                let entry = self.signatures.entry(key.1).or_insert(0);
                *entry += 1;
                *entry
            };
            if self.settings.signature_stop > 0 && hits >= self.settings.signature_stop {
                self.stuck = Some(format!(
                    "loop: the same failure result repeated {hits} times this turn"
                ));
            } else if self.settings.signature_warn > 0 && hits == self.settings.signature_warn {
                self.strained = true;
                return format!(
                    "[loop: this failure result has occurred {hits} times this turn across \
                     different calls — state the hypothesis the next action tests before \
                     retrying]\n{output}"
                );
            }
        }
        if self.settings.cycles && self.stuck.is_none() {
            self.detect_cycle(name);
        }
        if count < WARN {
            return output;
        }
        self.strained = true;
        let note = format!(
            "[loop: this exact call returned this same result {count} times in the last {} calls; \
             it will not change. Stop repeating it: re-read the error, try a different fix, or ask.]",
            self.seen.len()
        );
        format!("{note}\n{output}")
    }

    fn detect_cycle(&mut self, name: &str) {
        let len = self.seen.len();
        for period in 2..=CYCLE_MAX_PERIOD.min(len / CYCLE_MIN_REPS) {
            let need = period * CYCLE_MIN_REPS;
            if self
                .seen
                .iter()
                .skip(len - need)
                .enumerate()
                .all(|(index, key)| {
                    Some(*key) == self.seen.get(len - need + index % period).copied()
                })
            {
                self.strained = true;
                self.stuck = Some(format!(
                    "loop: a {period}-call cycle around `{name}` repeated {CYCLE_MIN_REPS} \
                     times with identical results"
                ));
                return;
            }
        }
    }

    pub(super) fn steered(&self) -> bool {
        self.steered
    }

    pub(super) fn mark_steered(&mut self) {
        self.steered = true;
        self.seen.clear();
        self.stuck = None;
    }

    pub(super) fn forgive(&mut self) {
        self.seen.clear();
        self.stuck = None;
        self.signatures.clear();
    }

    pub(super) fn strained(&self) -> bool {
        self.strained && !self.rejected
    }

    pub(super) fn reject(&mut self) {
        self.rejected = true;
    }

    pub(super) fn stuck(&self) -> Option<String> {
        self.stuck.clone()
    }
}

fn hash(value: &impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut digits = false;
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        for ch in word.chars() {
            if ch.is_ascii_digit() {
                if !digits {
                    out.push('#');
                }
                digits = true;
            } else {
                out.push(ch);
                digits = false;
            }
        }
        digits = false;
    }
    out
}

#[cfg(test)]
#[path = "guard_tests.rs"]
mod tests;
