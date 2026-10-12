use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::preset::{Preset, presets};
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Region {
    Intl,
    Cn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Access {
    Key,
    Plan,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Choice {
    #[serde(default)]
    pub(crate) region: Option<Region>,
    #[serde(default)]
    pub(crate) access: Option<Access>,
}

impl Region {
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Intl => "international",
            Self::Cn => "China",
        }
    }
}

impl Access {
    #[must_use]
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Key => "API key",
            Self::Plan => "coding plan",
        }
    }
}

impl Preset {
    pub(super) const fn access(&self) -> Access {
        if self.plan { Access::Plan } else { Access::Key }
    }

    fn fits(&self, choice: Choice) -> bool {
        choice
            .region
            .is_none_or(|region| self.region == Some(region))
            && choice.access.is_none_or(|access| self.access() == access)
    }
}

fn retired() -> &'static BTreeMap<&'static str, &'static str> {
    static INDEX: OnceLock<BTreeMap<&'static str, &'static str>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut index = BTreeMap::new();
        for preset in presets() {
            for old in &preset.replaces {
                index.insert(old.as_str(), preset.name.as_str());
            }
        }
        index
    })
}

pub(super) fn routes(provider: &str) -> impl Iterator<Item = &'static Preset> + '_ {
    presets().filter(move |preset| preset.provider == provider)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Route {
    pub(crate) name: String,
    pub(crate) region: Option<Region>,
    pub(crate) access: Access,
}

pub(super) fn options(provider: &str) -> Vec<Route> {
    routes(provider)
        .map(|preset| Route {
            name: preset.name.clone(),
            region: preset.region,
            access: preset.access(),
        })
        .collect()
}

pub(super) fn provider_of(route: &str) -> &str {
    preset(route).map_or(route, |preset| preset.provider.as_str())
}

pub(super) fn pick(file: &File, name: &str) -> String {
    let owned = file
        .endpoint
        .get(name)
        .is_some_and(|row| row.base_url.is_some() && preset(name).is_none());
    if owned {
        return name.to_string();
    }
    if let Some(route) = retired().get(name) {
        return (*route).to_string();
    }
    let options: Vec<&Preset> = routes(name).collect();
    if options.len() < 2 {
        return name.to_string();
    }
    let chosen = file
        .provider
        .get(name)
        .and_then(|choice| options.iter().find(|route| route.fits(*choice)));
    chosen
        .or_else(|| options.iter().find(|route| ready(file, route)))
        .or_else(|| options.first())
        .map_or_else(|| name.to_string(), |route| route.name.clone())
}

fn ready(file: &File, route: &Preset) -> bool {
    let configured = file
        .endpoint
        .get(&route.name)
        .and_then(|row| row.api_key.as_deref())
        .is_some_and(|key| !key.trim().is_empty());
    let stored = |slot: &str| secret::get(slot).ok().flatten().is_some();
    configured
        || env_key(route.env_keys).is_some()
        || stored(&route.name)
        || route.key_from.as_deref().is_some_and(stored)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Toggle {
    Region,
    Access,
}

pub(super) fn next(info: &super::AuthInfo, toggle: Toggle) -> Option<&Route> {
    let current = info.routes.iter().find(|route| route.name == info.name)?;
    let differs = |route: &&Route| match toggle {
        Toggle::Region => route.region != current.region,
        Toggle::Access => route.access != current.access,
    };
    let keeps = |route: &&Route| match toggle {
        Toggle::Region => route.access == current.access,
        Toggle::Access => route.region == current.region,
    };
    let candidates: Vec<&Route> = info.routes.iter().filter(differs).collect();
    candidates
        .iter()
        .find(|route| keeps(route))
        .or_else(|| candidates.first())
        .copied()
}

pub(crate) fn toggle(info: &super::AuthInfo, toggle: Toggle) -> Result<Option<String>> {
    let Some(next) = next(info, toggle) else {
        return Ok(None);
    };
    save(
        &info.provider,
        Choice {
            region: next.region,
            access: Some(next.access),
        },
    )?;
    Ok(Some(next.name.clone()))
}

fn save(provider: &str, choice: Choice) -> Result<()> {
    let mut fields = Vec::new();
    if let Some(access) = choice.access {
        fields.push(("access", word(access)?));
    }
    if let Some(region) = choice.region {
        fields.push(("region", word(region)?));
    }
    let fields: Vec<(&str, &str)> = fields
        .iter()
        .map(|(key, setting)| (*key, setting.as_str()))
        .collect();
    config::save_provider(&config::path()?, provider, &fields)
}

pub(super) fn choice_for(route: &str) -> Option<(String, Choice)> {
    let preset = preset(route)?;
    if preset.provider == route || routes(&preset.provider).nth(1).is_none() {
        return None;
    }
    Some((
        preset.provider.clone(),
        Choice {
            region: preset.region,
            access: Some(preset.access()),
        },
    ))
}

pub(crate) fn select(route: &str) -> Result<bool> {
    let Some((provider, choice)) = choice_for(route) else {
        return Ok(false);
    };
    save(&provider, choice)?;
    Ok(true)
}

pub(super) fn slot(file: &File, name: &str) -> String {
    if preset(name).is_some() {
        name.to_string()
    } else {
        pick(file, name)
    }
}

pub(super) fn retired_name(name: &str) -> bool {
    retired().contains_key(name)
}

fn word(setting: impl Serialize) -> Result<String> {
    Ok(serde_json::to_value(setting)?
        .as_str()
        .unwrap_or_default()
        .to_string())
}

pub(crate) fn provider_names(file: &File) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut names = Vec::new();
    for preset in presets() {
        if seen.insert(preset.provider.clone()) {
            names.push(preset.provider.clone());
        }
    }
    for (name, row) in &file.endpoint {
        let builtin = preset(name).is_some() || retired().contains_key(name.as_str());
        let owned = row.base_url.is_some() && preset(name).is_none();
        if (!builtin || owned) && seen.insert(name.clone()) {
            names.push(name.clone());
        }
    }
    names
}

#[cfg(test)]
#[path = "route_tests.rs"]
mod tests;
