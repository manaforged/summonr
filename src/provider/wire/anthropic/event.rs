use std::borrow::Cow;

use serde::Deserialize;
use serde_json::value::RawValue;

use crate::provider::Usage;

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum EventKind {
    MessageStart,
    ContentBlockStart,
    ContentBlockDelta,
    ContentBlockStop,
    MessageDelta,
    MessageStop,
    Error,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Default, Deserialize)]
pub(super) struct Event<'a> {
    #[serde(default, rename = "type")]
    pub(super) kind: EventKind,
    #[serde(default)]
    pub(super) message: Started,
    #[serde(borrow, default)]
    pub(super) content_block: Option<&'a RawValue>,
    #[serde(borrow, default)]
    pub(super) delta: Change<'a>,
    #[serde(default)]
    pub(super) usage: Option<Counted>,
    #[serde(borrow, default)]
    pub(super) error: Failure<'a>,
}

#[derive(Default, Deserialize)]
pub(super) struct Started {
    #[serde(default)]
    pub(super) usage: Option<Counted>,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Shape {
    Text,
    Thinking,
    ToolUse,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Default, Deserialize)]
pub(super) struct Opened<'a> {
    #[serde(default, rename = "type")]
    pub(super) shape: Shape,
    #[serde(borrow, default)]
    pub(super) text: Cow<'a, str>,
    #[serde(borrow, default)]
    pub(super) thinking: Cow<'a, str>,
    #[serde(borrow, default)]
    pub(super) signature: Cow<'a, str>,
    #[serde(borrow, default)]
    pub(super) id: Cow<'a, str>,
    #[serde(borrow, default)]
    pub(super) name: Cow<'a, str>,
}

#[derive(Default, Deserialize)]
pub(super) struct Change<'a> {
    #[serde(borrow, default)]
    pub(super) text: Cow<'a, str>,
    #[serde(borrow, default)]
    pub(super) thinking: Cow<'a, str>,
    #[serde(borrow, default)]
    pub(super) signature: Cow<'a, str>,
    #[serde(borrow, default)]
    pub(super) partial_json: Cow<'a, str>,
    #[serde(borrow, default)]
    pub(super) stop_reason: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(super) stop_details: Option<Details<'a>>,
}

#[derive(Default, Deserialize)]
pub(super) struct Details<'a> {
    #[serde(borrow, default)]
    pub(super) explanation: Cow<'a, str>,
}

#[derive(Default, Deserialize)]
pub(super) struct Failure<'a> {
    #[serde(borrow, default, rename = "type")]
    pub(super) kind: Cow<'a, str>,
    #[serde(borrow, default)]
    pub(super) message: Cow<'a, str>,
}

#[derive(Clone, Copy, Default, Deserialize)]
pub(crate) struct Counted {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    output_tokens_details: Option<Thought>,
}

#[derive(Clone, Copy, Default, Deserialize)]
struct Thought {
    #[serde(default)]
    thinking_tokens: Option<u64>,
}

impl Counted {
    pub(crate) fn merge(&mut self, newer: Self) {
        let keep = |old: &mut Option<u64>, new: Option<u64>| {
            if new.is_some() {
                *old = new;
            }
        };
        keep(&mut self.input_tokens, newer.input_tokens);
        keep(
            &mut self.cache_creation_input_tokens,
            newer.cache_creation_input_tokens,
        );
        keep(
            &mut self.cache_read_input_tokens,
            newer.cache_read_input_tokens,
        );
        keep(&mut self.output_tokens, newer.output_tokens);
        if newer.output_tokens_details.is_some() {
            self.output_tokens_details = newer.output_tokens_details;
        }
    }

    pub(crate) fn usage(&self) -> Usage {
        let field = |count: Option<u64>| Usage::clamp(count.unwrap_or_default());
        Usage {
            prompt_tokens: field(self.input_tokens)
                + field(self.cache_creation_input_tokens)
                + field(self.cache_read_input_tokens),
            completion_tokens: field(self.output_tokens),
            reasoning_tokens: field(
                self.output_tokens_details
                    .and_then(|details| details.thinking_tokens),
            ),
            cached_tokens: field(self.cache_read_input_tokens),
            cache_write_tokens: field(self.cache_creation_input_tokens),
        }
    }
}
