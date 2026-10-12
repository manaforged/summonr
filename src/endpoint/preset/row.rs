use std::collections::BTreeMap;

use serde::Deserialize;

use super::*;
use crate::provider::levels::STANDARD;
use crate::provider::{Effort, EffortField, Off, Scale};

#[derive(Deserialize)]
pub(super) struct File {
    pub(super) preset: Vec<Row>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Row {
    pub(super) name: String,
    pub(super) base_url: String,
    #[serde(default)]
    models_url: Option<String>,
    #[serde(default)]
    model: String,
    #[serde(default)]
    env_keys: Vec<String>,
    #[serde(default)]
    default_key: Option<String>,
    #[serde(default)]
    missing: Option<String>,
    #[serde(default)]
    aliases: BTreeMap<String, String>,
    #[serde(default)]
    oauth: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    wire: Option<String>,
    #[serde(default)]
    reasoning: ReasoningRow,
    #[serde(default)]
    context_window: Option<u32>,
    #[serde(default)]
    opencode: bool,
    #[serde(default)]
    prompt_cache_key: bool,
    #[serde(default)]
    assistant_after_tool: bool,
    #[serde(default)]
    cache_ttl: Option<CacheTtl>,
    #[serde(default)]
    cache_header: Option<String>,
    #[serde(default)]
    cli_token: Option<String>,
    #[serde(default = "yes")]
    opencode_oauth: bool,
    #[serde(default)]
    env_default: bool,
    #[serde(default)]
    plan: bool,
    #[serde(default)]
    off: bool,
    #[serde(default)]
    opencode_ids: Vec<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    start: Option<String>,
    #[serde(default)]
    key_from: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    region: Option<super::super::Region>,
    #[serde(default)]
    replaces: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ReasoningRow {
    #[serde(default)]
    field: Option<String>,
    #[serde(default)]
    levels: Option<Vec<Effort>>,
    #[serde(default)]
    off: Option<Off>,
    #[serde(default)]
    host_only: bool,
}

fn yes() -> bool {
    true
}

type Checked<T> = std::result::Result<T, String>;

fn leak_str(value: Option<&str>) -> Option<&'static str> {
    value.map(|text| Box::leak(text.to_string().into_boxed_str()) as &'static str)
}

fn leak_list(values: &[String]) -> &'static [&'static str] {
    let kept: Vec<&'static str> = values
        .iter()
        .map(|value| Box::leak(value.clone().into_boxed_str()) as &'static str)
        .collect();
    Box::leak(kept.into_boxed_slice())
}

fn leak_pairs(pairs: BTreeMap<String, String>) -> &'static [(String, String)] {
    Box::leak(
        pairs
            .into_iter()
            .collect::<Vec<(String, String)>>()
            .into_boxed_slice(),
    )
}

fn env_key_ok(key: &str) -> bool {
    key.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !key.is_empty()
}

impl ReasoningRow {
    fn spec(self, name: &str) -> Checked<Spec> {
        let field = match self.field.as_deref() {
            None | Some("reasoning_effort") => EffortField::ReasoningEffort,
            Some("reasoning_object") => EffortField::ReasoningObject,
            Some(other) => {
                return Err(format!("`{name}` names unknown effort field `{other}`"));
            }
        };
        let mut levels = self.levels.unwrap_or_else(|| STANDARD.to_vec());
        levels.sort();
        levels.dedup();
        let off = self.off.unwrap_or(Off::Omit);
        Ok(Spec {
            field,
            scale: Scale::new(Box::leak(levels.into_boxed_slice()), off),
            by_model: !self.host_only,
        })
    }
}

impl Row {
    fn oauth(&self, name: &str) -> Checked<Option<fn() -> crate::config::OAuth>> {
        self.oauth
            .as_deref()
            .map(|named| {
                login(named).ok_or_else(|| format!("`{name}` names unknown login `{named}`"))
            })
            .transpose()
    }

    fn cli_token(&self, name: &str) -> Checked<Option<CliToken>> {
        self.cli_token
            .as_deref()
            .map(|named| {
                cli_token(named)
                    .ok_or_else(|| format!("`{name}` names unknown cli token `{named}`"))
            })
            .transpose()
    }

    fn wire(&self, name: &str) -> Checked<Wire> {
        match self.wire.as_deref() {
            None => Ok(Wire::Completions),
            Some(named) => {
                wire(named).map_err(|error| format!("`{name}` has an invalid wire: {error}"))
            }
        }
    }

    fn check_env_keys(&self, name: &str) -> Checked<()> {
        match self.env_keys.iter().find(|key| !env_key_ok(key)) {
            Some(key) => Err(format!("`{name}` lists env key `{key}`")),
            None => Ok(()),
        }
    }

    fn missing(&self, name: &str) -> String {
        self.missing.clone().unwrap_or_else(|| {
            if self.env_keys.is_empty() {
                format!("run `summonr login {name}`")
            } else {
                format!("run `summonr login {name}` or set {}", self.env_keys[0])
            }
        })
    }

    pub(super) fn into_preset(self, name: String, base_url: String) -> Checked<Preset> {
        let oauth = self.oauth(&name)?;
        let cli_token = self.cli_token(&name)?;
        let wire = self.wire(&name)?;
        self.check_env_keys(&name)?;
        let missing = self.missing(&name);
        let reasoning = self.reasoning.spec(&name)?;
        if self.key_from.as_deref() == Some(name.as_str()) {
            return Err(format!("`{name}` reads its key from itself"));
        }
        Ok(Preset {
            models_url: leak_str(self.models_url.as_deref()),
            model: self.model,
            env_keys: leak_list(&self.env_keys),
            default_key: leak_str(self.default_key.as_deref()),
            missing,
            aliases: leak_pairs(self.aliases),
            oauth,
            headers: leak_pairs(self.headers),
            wire,
            reasoning,
            context_window: self.context_window,
            opencode: self.opencode,
            prompt_cache_key: self.prompt_cache_key,
            assistant_after_tool: self.assistant_after_tool,
            cache_ttl: self.cache_ttl,
            cache_header: self.cache_header,
            cli_token,
            opencode_oauth: self.opencode_oauth,
            env_default: self.env_default,
            plan: self.plan,
            off: self.off,
            opencode_ids: leak_list(&self.opencode_ids),
            label: self.label,
            start: self.start,
            key_from: self.key_from,
            provider: self.provider.unwrap_or_else(|| name.clone()),
            region: self.region,
            replaces: self.replaces,
            name,
            base_url,
        })
    }
}
