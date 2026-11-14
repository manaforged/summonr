use serde::Deserialize;
use serde::de::Error as _;
use std::collections::BTreeMap;

use super::{Dialect, Hook, HookEvent, Hooks, Matcher, seconds};

#[derive(Deserialize)]
#[serde(untagged)]
enum HookEntry {
    Command(String),
    Table {
        command: String,
        #[serde(default)]
        dialect: Dialect,
        #[serde(default)]
        matcher: Option<String>,
        #[serde(default)]
        timeout: Option<f64>,
        #[serde(default, rename = "async")]
        background: bool,
    },
}

impl HookEntry {
    fn hook(self) -> Hook {
        match self {
            Self::Command(command) => Hook::plain(command, Dialect::Summonr),
            Self::Table {
                command,
                dialect,
                matcher,
                timeout,
                background,
            } => Hook {
                matcher: matcher.as_deref().and_then(Matcher::parse),
                timeout: timeout.and_then(seconds),
                background,
                ..Hook::plain(command, dialect)
            },
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Commands {
    One(HookEntry),
    Many(Vec<HookEntry>),
}

impl<'de> Deserialize<'de> for Hooks {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = BTreeMap::<String, serde_json::Value>::deserialize(deserializer)?;
        let mut hooks = Self::default();
        for event in HookEvent::ALL {
            let Some(value) = raw.get(event.key()) else {
                continue;
            };
            let list = match Commands::deserialize(value).map_err(D::Error::custom)? {
                Commands::One(entry) => vec![entry],
                Commands::Many(entries) => entries,
            };
            for entry in list {
                hooks.push(event, entry.hook());
            }
        }
        Ok(hooks)
    }
}
