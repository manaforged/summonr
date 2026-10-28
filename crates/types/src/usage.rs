use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(from = "UsageWire")]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub reasoning_tokens: u32,
    #[serde(default)]
    pub cached_tokens: u32,
    #[serde(default)]
    pub cache_write_tokens: u32,
}

#[derive(Deserialize)]
struct UsageWire {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: Option<u32>,
    #[serde(default)]
    reasoning_tokens: u32,
    #[serde(default)]
    cached_tokens: u32,
    #[serde(default)]
    cache_write_tokens: u32,
    #[serde(default)]
    prompt_cache_hit_tokens: u32,
    #[serde(default)]
    prompt_tokens_details: Option<PromptDetails>,
    #[serde(default)]
    completion_tokens_details: Option<CompletionDetails>,
}

#[derive(Deserialize)]
struct PromptDetails {
    #[serde(default)]
    cached_tokens: u32,
    #[serde(default, alias = "cache_creation_input_tokens")]
    cache_write_tokens: u32,
}

#[derive(Deserialize)]
struct CompletionDetails {
    #[serde(default)]
    reasoning_tokens: u32,
}

impl From<UsageWire> for Usage {
    fn from(wire: UsageWire) -> Self {
        let (nested, written) = wire
            .prompt_tokens_details
            .map(|details| (details.cached_tokens, details.cache_write_tokens))
            .unwrap_or_default();
        let reasoning = wire
            .completion_tokens_details
            .map(|details| details.reasoning_tokens)
            .unwrap_or_default()
            .max(wire.reasoning_tokens);
        let apart = reasoning > 0
            && wire.total_tokens
                == Some(
                    wire.prompt_tokens
                        .saturating_add(wire.completion_tokens)
                        .saturating_add(reasoning),
                );
        Self {
            prompt_tokens: wire.prompt_tokens,
            completion_tokens: if apart {
                wire.completion_tokens.saturating_add(reasoning)
            } else {
                wire.completion_tokens
            },
            reasoning_tokens: reasoning,
            cached_tokens: nested
                .max(wire.cached_tokens)
                .max(wire.prompt_cache_hit_tokens),
            cache_write_tokens: written.max(wire.cache_write_tokens),
        }
    }
}

impl Usage {
    #[must_use]
    pub fn clamp(count: u64) -> u32 {
        u32::try_from(count).unwrap_or(u32::MAX)
    }

    pub fn counted(
        prompt: u64,
        completion: u64,
        reasoning: u64,
        cached: u64,
        written: u64,
    ) -> Self {
        Self {
            prompt_tokens: Self::clamp(prompt),
            completion_tokens: Self::clamp(completion),
            reasoning_tokens: Self::clamp(reasoning),
            cached_tokens: Self::clamp(cached),
            cache_write_tokens: Self::clamp(written),
        }
    }

    pub fn add(&mut self, other: &Self) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(other.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(other.completion_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
        self.cached_tokens = self.cached_tokens.saturating_add(other.cached_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(other.cache_write_tokens);
    }
}
