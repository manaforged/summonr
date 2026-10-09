use serde::Deserialize;

use super::{Dialect, Hook, HookEvent, Hooks};

#[derive(Deserialize)]
#[serde(untagged)]
enum HookEntry {
    Command(String),
    Table {
        command: String,
        #[serde(default)]
        dialect: Dialect,
    },
}

impl HookEntry {
    fn hook(self) -> Hook {
        match self {
            Self::Command(command) => Hook::plain(command, Dialect::Summonr),
            Self::Table { command, dialect } => Hook::plain(command, dialect),
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
        #[derive(Deserialize)]
        struct Raw {
            pre_tool: Option<Commands>,
            post_tool: Option<Commands>,
            compact: Option<Commands>,
            stop: Option<Commands>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let mut hooks = Self::default();
        for (event, commands) in [
            (HookEvent::PreTool, raw.pre_tool),
            (HookEvent::PostTool, raw.post_tool),
            (HookEvent::Compact, raw.compact),
            (HookEvent::Stop, raw.stop),
        ] {
            let list = match commands {
                Some(Commands::One(entry)) => vec![entry],
                Some(Commands::Many(entries)) => entries,
                None => Vec::new(),
            };
            for entry in list {
                hooks.push(event, entry.hook());
            }
        }
        Ok(hooks)
    }
}
