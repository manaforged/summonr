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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hotkeys {
    keys: [char; Hotkey::ALL.len()],
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            keys: [
                'o', 'j', 'g', 'e', 'l', 'a', 'x', 'b', 't', 'p', 'f', 'r', 'n',
            ],
        }
    }
}

impl Hotkeys {
    pub(crate) fn configured(rows: &BTreeMap<String, String>) -> Result<Self> {
        let mut hotkeys = Self::assigned(rows)?;
        if hotkeys.keys.iter().filter(|key| **key == 'y').count() > 1 {
            return Err(Error::Config(
                "config.toml: hotkey ctrl+y is assigned more than once".to_string(),
            ));
        }
        for action in Hotkey::ALL {
            hotkeys.settle(action, rows)?;
        }
        hotkeys.unique()?;
        Ok(hotkeys)
    }

    fn assigned(rows: &BTreeMap<String, String>) -> Result<Self> {
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
        Ok(hotkeys)
    }

    fn settle(&mut self, action: Hotkey, rows: &BTreeMap<String, String>) -> Result<()> {
        let index = action.index();
        let inherited = matches!(
            action,
            Hotkey::Copy | Hotkey::Search | Hotkey::History | Hotkey::Fold
        ) && !rows.contains_key(action.name());
        if self.keys[index] == 'y' || inherited && self.shared(index) {
            self.keys[index] = Self::default()
                .keys
                .into_iter()
                .chain('a'..='z')
                .find(|key| available(*key) && !self.keys.contains(key))
                .ok_or_else(|| Error::Config(format!("no available {} hotkey", action.name())))?;
        }
        Ok(())
    }

    fn shared(&self, index: usize) -> bool {
        self.keys
            .iter()
            .enumerate()
            .any(|(other, key)| other != index && *key == self.keys[index])
    }

    fn unique(&self) -> Result<()> {
        for (index, key) in self.keys.iter().enumerate() {
            if self.keys[..index].contains(key) {
                return Err(Error::Config(format!(
                    "config.toml: hotkey ctrl+{key} is assigned more than once"
                )));
            }
        }
        Ok(())
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
    key.is_ascii_lowercase()
        && !matches!(
            key,
            'c' | 'd' | 'h' | 'i' | 'm' | 'q' | 's' | 'v' | 'y' | 'z'
        )
        && EditorKey::of(key).is_none_or(EditorKey::shared)
}

fn parse(binding: &str) -> Option<char> {
    let mut chars = binding.strip_prefix("ctrl+")?.chars();
    let key = chars.next()?;
    (chars.next().is_none() && (available(key) || key == 'y')).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::{EditorKey, Hotkey, Hotkeys, available};

    #[test]
    fn editor_line_keys_cannot_be_bound() {
        for key in ['u', 'k', 'w'] {
            assert!(!available(key));
            assert!(!Hotkeys::default().assign(Hotkey::Fold, key));
        }
        assert!(available('a') && available('x'));
    }

    #[test]
    fn select_all_and_cut_reach_a_non_empty_composer_off_macos() {
        let off_mac = !cfg!(target_os = "macos");
        assert_eq!(EditorKey::yields('a', false), off_mac);
        assert_eq!(EditorKey::yields('x', false), off_mac);
        assert!(!EditorKey::yields('a', true));
        assert!(!EditorKey::yields('o', false));
    }
}
