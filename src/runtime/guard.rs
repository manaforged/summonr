use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const WINDOW: usize = 24;
const WARN: usize = 3;
const STOP: usize = 6;

#[derive(Default)]
pub(super) struct Guard {
    seen: VecDeque<(u64, u64)>,
    stuck: Option<String>,
    failures: usize,
    strained: bool,
    rejected: bool,
}

impl Guard {
    pub(super) fn observe(
        &mut self,
        name: &str,
        arguments: &str,
        success: bool,
        output: String,
    ) -> String {
        self.failures = if success { 0 } else { self.failures + 1 };
        if self.failures >= WARN {
            self.strained = true;
        }
        let key = (hash(&(name, arguments)), hash(&normalize(&output)));
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
        if count < WARN {
            return output;
        }
        self.strained = true;
        format!(
            "[loop: this exact call returned this same result {count} times in the last {} calls; \
             it will not change. Stop repeating it: re-read the error, try a different fix, or ask. \
             The turn stops at {STOP}.]\n{output}",
            self.seen.len()
        )
    }

    pub(super) fn forgive(&mut self) {
        self.seen.clear();
        self.stuck = None;
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
