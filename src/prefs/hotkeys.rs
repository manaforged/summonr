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
    Fold,
}

impl Hotkey {
    pub(crate) const ALL: [Self; 13] = [
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
        Self::Fold,
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
            Self::Fold => "fold",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }

    fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.name() == name)
    }
}

type Map = ([char; Hotkey::ALL.len()], [bool; Hotkey::ALL.len()]);

const FORMER: [Map; 2] = [
    (
        [
            'o', 'j', 'g', 'e', 'l', 'a', 'x', 'b', 't', 'p', 'f', 'r', 'n',
        ],
        [false; Hotkey::ALL.len()],
    ),
    (
        [
            'o', 'j', 'g', 'e', 't', 'a', 'd', 'y', 't', 'b', 'f', 'r', 'n',
        ],
        [
            true, false, false, true, false, true, true, true, true, true, true, false, true,
        ],
    ),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hotkeys {
    keys: [char; Hotkey::ALL.len()],
    leader: [bool; Hotkey::ALL.len()],
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            keys: [
                'o', 'j', 'g', 'e', 't', 'a', 'd', 'y', 't', 'b', 'f', 'r', 'n',
            ],
            leader: [
                false, false, false, true, false, true, true, true, true, true, true, false, true,
            ],
        }
    }
}

impl Hotkeys {
    pub(crate) fn configured(rows: &BTreeMap<String, String>) -> Result<Self> {
        if previous_default(rows) {
            return Ok(Self::default());
        }
        let mut hotkeys = Self::assigned(rows)?;
        if hotkeys.bound('y', false) > 1 {
            return Err(Error::Config(
                "config.toml: hotkey ctrl+y is assigned more than once".to_string(),
            ));
        }
        hotkeys.resolve(rows)?;
        hotkeys.unique()?;
        Ok(hotkeys)
    }

    fn assigned(rows: &BTreeMap<String, String>) -> Result<Self> {
        let mut hotkeys = Self::default();
        for (name, binding) in rows {
            let action = Hotkey::named(name)
                .ok_or_else(|| Error::Config(format!("config.toml: unknown hotkey `{name}`")))?;
            let (key, leader) = parse(binding).ok_or_else(|| {
                Error::Config(format!(
                    "config.toml: hotkey `{name}` must be ctrl+letter or ctrl+x plus a letter"
                ))
            })?;
            let index = action.index();
            hotkeys.keys[index] = key;
            hotkeys.leader[index] = leader;
        }
        Ok(hotkeys)
    }

    fn resolve(&mut self, rows: &BTreeMap<String, String>) -> Result<()> {
        for action in Hotkey::ALL {
            if !self.displaced(action, rows) {
                continue;
            }
            let index = action.index();
            self.keys[index] = self
                .vacant(self.leader[index])
                .ok_or_else(|| Error::Config(format!("no available {} hotkey", action.name())))?;
        }
        Ok(())
    }

    fn displaced(&self, action: Hotkey, rows: &BTreeMap<String, String>) -> bool {
        let index = action.index();
        let bare_y = !self.leader[index] && self.keys[index] == 'y';
        let inherited = matches!(
            action,
            Hotkey::Copy | Hotkey::Search | Hotkey::History | Hotkey::Fold
        ) && !rows.contains_key(action.name());
        bare_y || inherited && self.bound(self.keys[index], self.leader[index]) > 1
    }

    fn vacant(&self, leader: bool) -> Option<char> {
        Self::default()
            .keys
            .into_iter()
            .chain('a'..='z')
            .find(|key| acceptable(*key, leader) && self.bound(*key, leader) == 0)
    }

    fn bound(&self, key: char, leader: bool) -> usize {
        self.keys
            .iter()
            .zip(self.leader)
            .filter(|(bound, plane)| **bound == key && *plane == leader)
            .count()
    }

    fn unique(&self) -> Result<()> {
        match self
            .keys
            .iter()
            .zip(self.leader)
            .find(|(key, leader)| self.bound(**key, *leader) > 1)
        {
            Some((key, leader)) => Err(Error::Config(format!(
                "config.toml: hotkey {} is assigned more than once",
                label_of(leader, *key)
            ))),
            None => Ok(()),
        }
    }

    #[must_use]
    pub(crate) const fn key(&self, action: Hotkey) -> char {
        self.keys[action.index()]
    }

    #[must_use]
    pub(crate) fn label(&self, action: Hotkey) -> String {
        label_of(self.leader[action.index()], self.key(action))
    }

    #[must_use]
    pub(crate) fn action(&self, key: char) -> Option<Hotkey> {
        self.find(key, false)
    }

    #[must_use]
    pub(crate) fn chord(&self, key: char) -> Option<Hotkey> {
        self.find(key, true)
    }

    fn find(&self, key: char, leader: bool) -> Option<Hotkey> {
        Hotkey::ALL
            .into_iter()
            .find(|action| self.key(*action) == key && self.leader[action.index()] == leader)
    }

    pub(crate) fn assign(&mut self, action: Hotkey, key: char, leader: bool) -> bool {
        if !acceptable(key, leader) {
            return false;
        }
        let old_key = self.key(action);
        let old_leader = self.leader[action.index()];
        if let Some(other) = self.find(key, leader)
            && other != action
        {
            self.keys[other.index()] = old_key;
            self.leader[other.index()] = old_leader;
        }
        let index = action.index();
        self.keys[index] = key;
        self.leader[index] = leader;
        true
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = (Hotkey, String)> + '_ {
        Hotkey::ALL
            .into_iter()
            .map(|action| (action, self.label(action)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorKey {
    SelectAll,
    Cut,
    DeleteToStart,
    DeleteToEnd,
    DeleteWord,
}

impl EditorKey {
    pub(crate) const ALL: [Self; 5] = [
        Self::SelectAll,
        Self::Cut,
        Self::DeleteToStart,
        Self::DeleteToEnd,
        Self::DeleteWord,
    ];

    #[must_use]
    pub(crate) const fn key(self) -> char {
        match self {
            Self::SelectAll => 'a',
            Self::Cut => 'x',
            Self::DeleteToStart => 'u',
            Self::DeleteToEnd => 'k',
            Self::DeleteWord => 'w',
        }
    }

    #[must_use]
    pub(crate) const fn shared(self) -> bool {
        matches!(self, Self::SelectAll | Self::Cut)
    }

    #[must_use]
    pub(crate) fn of(key: char) -> Option<Self> {
        Self::ALL.into_iter().find(|editor| editor.key() == key)
    }

    #[must_use]
    pub(crate) fn yields(key: char, draft_empty: bool) -> bool {
        !cfg!(target_os = "macos") && !draft_empty && Self::of(key).is_some_and(Self::shared)
    }
}

#[must_use]
pub(crate) fn available(key: char) -> bool {
    loadable(key) && !matches!(key, 'a' | 'b' | 'e' | 'f' | 'l' | 'n' | 'p' | 'x')
}

fn loadable(key: char) -> bool {
    key.is_ascii_lowercase()
        && !matches!(
            key,
            'c' | 'd' | 'h' | 'i' | 'm' | 'q' | 's' | 'v' | 'y' | 'z'
        )
        && EditorKey::of(key).is_none_or(EditorKey::shared)
}

fn leader_letter(key: char) -> bool {
    key.is_ascii_lowercase() && key != 'x'
}

fn acceptable(key: char, leader: bool) -> bool {
    if leader {
        leader_letter(key)
    } else {
        available(key)
    }
}

fn label_of(leader: bool, key: char) -> String {
    if leader {
        format!("ctrl+x {key}")
    } else {
        format!("ctrl+{key}")
    }
}

fn previous_default(rows: &BTreeMap<String, String>) -> bool {
    rows.len() == Hotkey::ALL.len()
        && FORMER.iter().any(|(keys, leader)| {
            Hotkey::ALL.into_iter().all(|action| {
                let index = action.index();
                rows.get(action.name())
                    .is_some_and(|binding| *binding == label_of(leader[index], keys[index]))
            })
        })
}

fn parse(binding: &str) -> Option<(char, bool)> {
    let rest = binding.strip_prefix("ctrl+")?;
    if let Some(chord) = rest
        .strip_prefix("x ")
        .or_else(|| rest.strip_prefix('x').filter(|chord| !chord.is_empty()))
    {
        return one_letter(chord)
            .filter(|key| leader_letter(*key))
            .map(|key| (key, true));
    }
    one_letter(rest)
        .filter(|key| loadable(*key) || *key == 'y')
        .map(|key| (key, false))
}

fn one_letter(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let key = chars.next()?;
    chars.next().is_none().then_some(key)
}

#[cfg(test)]
#[path = "hotkeys_tests.rs"]
mod tests;
