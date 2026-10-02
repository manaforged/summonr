use std::collections::BTreeMap;

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Hotkey {
    Options,
    Newline,
    Editor,
    Files,
    Ledger,
    Agents,
    Diff,
    Copy,
    Select,
    Pane,
    Search,
    History,
}

impl Hotkey {
    pub(crate) const ALL: [Self; 12] = [
        Self::Options,
        Self::Newline,
        Self::Editor,
        Self::Files,
        Self::Ledger,
        Self::Agents,
        Self::Diff,
        Self::Copy,
        Self::Select,
        Self::Pane,
        Self::Search,
        Self::History,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Options => "options",
            Self::Newline => "newline",
            Self::Editor => "editor",
            Self::Files => "files",
            Self::Ledger => "ledger",
            Self::Agents => "agents",
            Self::Diff => "diff",
            Self::Copy => "copy",
            Self::Select => "select",
            Self::Pane => "pane",
            Self::Search => "search",
            Self::History => "history",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }

    fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.name() == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hotkeys {
    keys: [char; Hotkey::ALL.len()],
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            keys: ['o', 'j', 'g', 'e', 'l', 'a', 'x', 'b', 't', 'p', 'f', 'r'],
        }
    }
}

impl Hotkeys {
    pub(crate) fn configured(rows: &BTreeMap<String, String>) -> Result<Self> {
        let mut hotkeys = Self::default();
        for (name, binding) in rows {
            let action = Hotkey::named(name)
                .ok_or_else(|| Error::Config(format!("config.toml: unknown hotkey `{name}`")))?;
            let key = parse(binding).ok_or_else(|| {
                Error::Config(format!(
                    "config.toml: hotkey `{name}` must be an available ctrl+letter"
                ))
            })?;
            hotkeys.keys[action.index()] = key;
        }
        if hotkeys.keys.iter().filter(|key| **key == 'y').count() > 1 {
            return Err(Error::Config(
                "config.toml: hotkey ctrl+y is assigned more than once".to_string(),
            ));
        }
        for action in Hotkey::ALL {
            let index = action.index();
            let inherited = matches!(action, Hotkey::Copy | Hotkey::Search | Hotkey::History)
                && !rows.contains_key(action.name());
            if hotkeys.keys[index] == 'y'
                || inherited
                    && hotkeys
                        .keys
                        .iter()
                        .enumerate()
                        .any(|(other, key)| other != index && *key == hotkeys.keys[index])
            {
                hotkeys.keys[index] = Self::default()
                    .keys
                    .into_iter()
                    .chain('a'..='z')
                    .find(|key| available(*key) && !hotkeys.keys.contains(key))
                    .ok_or_else(|| {
                        Error::Config(format!("no available {} hotkey", action.name()))
                    })?;
            }
        }
        for (index, key) in hotkeys.keys.iter().enumerate() {
            if hotkeys.keys[..index].contains(key) {
                return Err(Error::Config(format!(
                    "config.toml: hotkey ctrl+{key} is assigned more than once"
                )));
            }
        }
        Ok(hotkeys)
    }

    #[must_use]
    pub(crate) const fn key(&self, action: Hotkey) -> char {
        self.keys[action.index()]
    }

    #[must_use]
    pub(crate) fn label(&self, action: Hotkey) -> String {
        format!("ctrl+{}", self.key(action))
    }

    #[must_use]
    pub(crate) fn action(&self, key: char) -> Option<Hotkey> {
        Hotkey::ALL
            .into_iter()
            .find(|action| self.key(*action) == key)
    }

    pub(crate) fn assign(&mut self, action: Hotkey, key: char) -> bool {
        if !available(key) {
            return false;
        }
        let old = self.key(action);
        if let Some(other) = self.action(key) {
            self.keys[other.index()] = old;
        }
        self.keys[action.index()] = key;
        true
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = (Hotkey, String)> + '_ {
        Hotkey::ALL
            .into_iter()
            .map(|action| (action, self.label(action)))
    }
}

#[must_use]
pub(crate) const fn available(key: char) -> bool {
    key.is_ascii_lowercase()
        && !matches!(
            key,
            'c' | 'd' | 'h' | 'i' | 'm' | 'q' | 's' | 'v' | 'y' | 'z'
        )
}

fn parse(binding: &str) -> Option<char> {
    let mut chars = binding.strip_prefix("ctrl+")?.chars();
    let key = chars.next()?;
    (chars.next().is_none() && (available(key) || key == 'y')).then_some(key)
}
