use std::borrow::Cow;

use serde::Deserialize;
use serde_json::Value;
use serde_json::value::RawValue;

use crate::provider::Usage;

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
pub(super) enum FrameKind {
    #[serde(rename = "response.output_text.delta")]
    TextDelta,
    #[serde(rename = "response.refusal.delta")]
    RefusalDelta,
    #[serde(rename = "response.reasoning_text.delta")]
    ReasoningDelta,
    #[serde(rename = "response.reasoning_summary_text.delta")]
    SummaryDelta,
    #[serde(rename = "response.output_item.added")]
    ItemAdded,
    #[serde(rename = "response.function_call_arguments.delta")]
    ArgumentsDelta,
    #[serde(rename = "response.output_item.done")]
    ItemDone,
    #[serde(rename = "response.failed")]
    Failed,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "response.completed")]
    Completed,
    #[serde(rename = "response.incomplete")]
    Incomplete,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Default, Deserialize)]
pub(super) struct Frame<'a> {
    #[serde(default, rename = "type")]
    pub(super) kind: FrameKind,
    #[serde(borrow, default)]
    pub(super) delta: Cow<'a, str>,
    #[serde(borrow, default)]
    pub(super) item_id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(super) item: Option<Item<'a>>,
    #[serde(borrow, default)]
    pub(super) response: Option<Response<'a>>,
    #[serde(borrow, default)]
    pub(super) error: Option<Failure<'a>>,
    #[serde(default)]
    pub(super) code: Option<Value>,
    #[serde(borrow, default)]
    pub(super) message: Option<Cow<'a, str>>,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Shape {
    Reasoning,
    Message,
    FunctionCall,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Default, Deserialize)]
pub(super) struct Item<'a> {
    #[serde(default, rename = "type")]
    pub(super) shape: Shape,
    #[serde(borrow, default)]
    pub(super) id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(super) call_id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(super) name: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(super) arguments: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(super) encrypted_content: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(super) phase: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(super) content: Option<Vec<Part<'a>>>,
}

impl Item<'_> {
    pub(super) fn key(&self) -> &str {
        self.id
            .as_deref()
            .or(self.call_id.as_deref())
            .unwrap_or_default()
    }

    pub(super) fn parts(&self) -> &[Part<'_>] {
        self.content.as_deref().unwrap_or_default()
    }
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum PartShape {
    OutputText,
    Refusal,
    #[default]
    #[serde(other)]
    Other,
}

#[derive(Default, Deserialize)]
pub(super) struct Part<'a> {
    #[serde(default, rename = "type")]
    pub(super) shape: PartShape,
    #[serde(borrow, default)]
    pub(super) text: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(super) refusal: Option<Cow<'a, str>>,
}

#[derive(Default, Deserialize)]
pub(super) struct Response<'a> {
    #[serde(borrow, default)]
    pub(super) status: Option<Cow<'a, str>>,
    #[serde(default)]
    pub(super) usage: Option<Counted>,
    #[serde(borrow, default)]
    pub(super) incomplete_details: Option<Details<'a>>,
    #[serde(borrow, default)]
    pub(super) output: Option<&'a RawValue>,
    #[serde(borrow, default)]
    pub(super) error: Option<Failure<'a>>,
}

#[derive(Default, Deserialize)]
pub(super) struct Details<'a> {
    #[serde(borrow, default)]
    pub(super) reason: Option<Cow<'a, str>>,
}

#[derive(Default, Deserialize)]
pub(super) struct Failure<'a> {
    #[serde(default)]
    pub(super) code: Option<Value>,
    #[serde(borrow, default)]
    pub(super) message: Option<Cow<'a, str>>,
}

#[derive(Clone, Copy, Default, Deserialize)]
pub(super) struct Counted {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    output_tokens_details: Option<Detail>,
    #[serde(default)]
    input_tokens_details: Option<Detail>,
}

#[derive(Clone, Copy, Default, Deserialize)]
struct Detail {
    #[serde(default)]
    reasoning_tokens: Option<u64>,
    #[serde(default)]
    cached_tokens: Option<u64>,
    #[serde(default)]
    cache_write_tokens: Option<u64>,
}

impl Counted {
    pub(super) fn usage(&self) -> Usage {
        let field = |count: Option<u64>| Usage::clamp(count.unwrap_or_default());
        Usage {
            prompt_tokens: field(self.input_tokens),
            completion_tokens: field(self.output_tokens),
            reasoning_tokens: field(
                self.output_tokens_details
                    .and_then(|detail| detail.reasoning_tokens),
            ),
            cached_tokens: field(
                self.input_tokens_details
                    .and_then(|detail| detail.cached_tokens),
            ),
            cache_write_tokens: field(
                self.input_tokens_details
                    .and_then(|detail| detail.cache_write_tokens),
            ),
        }
    }
}
