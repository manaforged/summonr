use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};

use super::*;
use crate::provider::levels::STANDARD;
use crate::provider::{Effort, EffortField, Off, Scale};

#[derive(Deserialize)]
pub(super) struct File {
    pub(super) preset: Vec<Preset>,
    #[serde(default)]
    pub(super) oauth: BTreeMap<String, crate::config::OAuth>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct ReasoningRow {
    #[serde(default)]
    field: Option<EffortField>,
    #[serde(default)]
    levels: Option<Vec<Effort>>,
    #[serde(default)]
    off: Option<Off>,
    #[serde(default)]
    host_only: bool,
}

type Checked<T> = std::result::Result<T, String>;

pub(super) fn wire<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Wire, D::Error> {
    let named = String::deserialize(deserializer)?;
    Wire::parse(&named).ok_or_else(|| serde::de::Error::custom(format!("unknown wire `{named}`")))
}

fn env_key_ok(key: &str) -> bool {
    key.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !key.is_empty()
}

impl ReasoningRow {
    fn spec(&self) -> Spec {
        let mut levels = self.levels.clone().unwrap_or_else(|| STANDARD.to_vec());
        levels.sort();
        levels.dedup();
        Spec {
            field: self.field.unwrap_or(EffortField::ReasoningEffort),
            scale: Scale::new(
                Box::leak(levels.into_boxed_slice()),
                self.off.unwrap_or(Off::Omit),
            ),
            by_model: !self.host_only,
        }
    }
}

impl Preset {
    pub(super) fn check(&mut self, logins: &BTreeMap<String, crate::config::OAuth>) -> Checked<()> {
        let name = self.name.trim().to_string();
        let base_url = self.base_url.trim().to_string();
        if name.is_empty() || base_url.is_empty() {
            return Err("every preset needs a name and a base_url".to_string());
        }
        self.oauth = self
            .login
            .as_deref()
            .map(|named| {
                logins
                    .get(named)
                    .cloned()
                    .ok_or_else(|| format!("`{name}` names unknown login `{named}`"))
            })
            .transpose()?;
        if let Some(key) = self
            .env_keys
            .iter()
            .chain(&self.env_unset)
            .find(|key| !env_key_ok(key))
        {
            return Err(format!("`{name}` lists env key `{key}`"));
        }
        if self.key_from.contains(&name) {
            return Err(format!("`{name}` reads its key from itself"));
        }
        if self.missing.is_empty() {
            self.missing = match self.env_keys.first() {
                None => format!("run `summonr login {name}`"),
                Some(key) => format!("run `summonr login {name}` or set {key}"),
            };
        }
        if self.provider.is_empty() {
            self.provider.clone_from(&name);
        }
        self.reasoning = self.reasoning_row.spec();
        self.name = name;
        self.base_url = base_url;
        Ok(())
    }
}
