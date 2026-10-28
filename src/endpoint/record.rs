use super::preset::Preset;
use super::*;
use crate::provider::Off;

#[derive(Clone, Default)]
pub(super) struct Record {
    pub(super) base_url: Option<String>,
    pub(super) models_url: Option<String>,
    pub(super) model: Option<String>,
    pub(super) api_key: Option<String>,
    pub(super) oauth: Option<crate::config::OAuth>,
    pub(super) body: Option<serde_json::Value>,
    pub(super) context_window: Option<u32>,
    pub(super) idle_timeout_secs: Option<u64>,
    pub(super) effort: Option<Effort>,
    pub(super) price: Option<Price>,
    pub(super) env: BTreeMap<String, String>,
    pub(super) headers: BTreeMap<String, String>,
    pub(super) wire: Option<Wire>,
    pub(super) fallback: Vec<String>,
    pub(super) compact_tokens: Option<u32>,
    pub(super) images: Option<bool>,
    pub(super) keep_reasoning: Option<bool>,
    pub(super) cache_ttl: Option<CacheTtl>,
    pub(super) reasoning_levels: Option<Vec<Effort>>,
    pub(super) thinking_off: Option<Off>,
}

impl Record {
    pub(super) fn merge(mut self, over: Self) -> Self {
        self.base_url = over.base_url.or(self.base_url);
        self.models_url = over.models_url.or(self.models_url);
        self.model = over.model.or(self.model);
        self.api_key = over.api_key.or(self.api_key);
        self.oauth = over.oauth.or(self.oauth);
        self.body = over.body.or(self.body);
        self.context_window = over.context_window.or(self.context_window);
        self.idle_timeout_secs = over.idle_timeout_secs.or(self.idle_timeout_secs);
        self.effort = over.effort.or(self.effort);
        self.price = over.price.or(self.price);
        self.env.extend(over.env);
        self.headers.extend(over.headers);
        self.wire = over.wire.or(self.wire);
        if !over.fallback.is_empty() {
            self.fallback = over.fallback;
        }
        self.compact_tokens = over.compact_tokens.or(self.compact_tokens);
        self.images = over.images.or(self.images);
        self.keep_reasoning = over.keep_reasoning.or(self.keep_reasoning);
        self.cache_ttl = over.cache_ttl.or(self.cache_ttl);
        self.reasoning_levels = over.reasoning_levels.or(self.reasoning_levels);
        self.thinking_off = over.thinking_off.or(self.thinking_off);
        self
    }

    pub(super) fn endpoint(self, name: &str, facts: Option<&Preset>) -> Endpoint {
        let mut reasoning = reasoning::from_preset(facts);
        reasoning::overlay(
            &mut reasoning,
            self.reasoning_levels.as_deref(),
            self.thinking_off,
        );
        Endpoint {
            name: name.to_string(),
            model: self.model.unwrap_or_default(),
            base_url: self.base_url,
            models_url: self.models_url,
            api_key: self.api_key,
            source: None,
            oauth: self.oauth,
            body: self.body,
            context_window: self.context_window,
            idle_timeout_secs: self.idle_timeout_secs,
            effort: self.effort,
            price: self.price,
            env: self.env,
            headers: self.headers,
            wire: self.wire.unwrap_or_default(),
            fallback: self.fallback,
            compact_tokens: self.compact_tokens,
            images: self.images,
            keep_reasoning: self.keep_reasoning.unwrap_or(false),
            opencode: facts.is_some_and(|preset| preset.opencode),
            prompt_cache_key: facts.is_some_and(|preset| preset.prompt_cache_key),
            assistant_after_tool: facts.is_some_and(|preset| preset.assistant_after_tool),
            cache_ttl: self
                .cache_ttl
                .or_else(|| facts.and_then(|preset| preset.cache_ttl))
                .unwrap_or(crate::limits::limits().cache_ttl),
            cache_header: facts.and_then(|preset| preset.cache_header.clone()),
            cache_marks: facts.is_some_and(|preset| preset.cache_marks),
            reasoning,
        }
    }
}

impl From<&Preset> for Record {
    fn from(row: &Preset) -> Self {
        Self {
            base_url: Some(row.base_url.clone()),
            models_url: row.models_url.clone(),
            model: Some(row.model.clone()),
            oauth: row.sign_in(),
            context_window: row.context_window,
            headers: row.headers.clone(),
            wire: Some(row.wire),
            ..Self::default()
        }
    }
}

impl From<&FileEndpoint> for Record {
    fn from(row: &FileEndpoint) -> Self {
        Self {
            base_url: row.base_url.as_deref().map(trim_slash),
            models_url: row.models_url.as_deref().map(trim_slash),
            model: row.model.clone(),
            api_key: row.api_key.clone().filter(|key| !key.trim().is_empty()),
            oauth: row.auth.clone(),
            body: row.body.as_ref().map(json),
            context_window: row.context_window,
            idle_timeout_secs: row.idle_timeout_secs,
            effort: row.effort,
            price: row.price,
            env: row.env.clone(),
            headers: row.headers.clone(),
            wire: row.wire,
            fallback: row.fallback.clone(),
            compact_tokens: row.compact_tokens,
            images: row.images,
            keep_reasoning: row.keep_reasoning,
            cache_ttl: row.cache_ttl,
            reasoning_levels: row.reasoning_levels.clone(),
            thinking_off: row.thinking_off,
        }
    }
}
