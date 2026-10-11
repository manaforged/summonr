use std::borrow::Cow;

use serde::Deserialize;
use serde_json::value::RawValue;

use crate::provider::Usage;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Chunk<'a> {
    #[serde(borrow, default)]
    pub(crate) error: Option<Failure<'a>>,
    #[serde(borrow, default)]
    pub(crate) prompt_feedback: Option<Feedback<'a>>,
    #[serde(borrow, default)]
    pub(crate) candidates: Vec<Candidate<'a>>,
    #[serde(default)]
    pub(crate) usage_metadata: Option<Counted>,
}

impl Chunk<'_> {
    pub(crate) fn finish_reason(&self) -> Option<&str> {
        self.candidates.first()?.finish_reason.as_deref()
    }

    pub(crate) fn parts(&self) -> &[Part<'_>] {
        self.candidates
            .first()
            .and_then(|candidate| candidate.content.as_ref())
            .map_or(&[], |content| content.parts.as_slice())
    }
}

#[derive(Deserialize)]
pub(crate) struct Failure<'a> {
    #[serde(default)]
    pub(crate) code: Option<u64>,
    #[serde(borrow, default)]
    pub(crate) status: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(crate) message: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Feedback<'a> {
    #[serde(borrow, default)]
    pub(crate) block_reason: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Candidate<'a> {
    #[serde(borrow, default)]
    content: Option<Content<'a>>,
    #[serde(borrow, default)]
    finish_reason: Option<Cow<'a, str>>,
}

#[derive(Deserialize)]
struct Content<'a> {
    #[serde(borrow, default)]
    parts: Vec<Part<'a>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Part<'a> {
    #[serde(borrow, default)]
    pub(crate) text: Option<Cow<'a, str>>,
    #[serde(default)]
    pub(crate) thought: Option<bool>,
    #[serde(borrow, default)]
    pub(crate) thought_signature: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(crate) function_call: Option<Call<'a>>,
}

#[derive(Deserialize)]
pub(crate) struct Call<'a> {
    #[serde(borrow, default)]
    pub(crate) name: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(crate) id: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub(crate) args: Option<&'a RawValue>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Counted {
    #[serde(default)]
    prompt_token_count: u64,
    #[serde(default)]
    candidates_token_count: u64,
    #[serde(default)]
    thoughts_token_count: u64,
    #[serde(default)]
    cached_content_token_count: u64,
}

impl Counted {
    pub(crate) fn usage(&self) -> Usage {
        let field = Usage::clamp;
        Usage {
            prompt_tokens: field(self.prompt_token_count),
            completion_tokens: field(self.candidates_token_count)
                + field(self.thoughts_token_count),
            reasoning_tokens: field(self.thoughts_token_count),
            cached_tokens: field(self.cached_content_token_count),
            cache_write_tokens: 0,
        }
    }
}
