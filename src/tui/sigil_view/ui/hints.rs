use sigil::prelude::{AlignItems, LayoutBuilderExt, NodeBuilder, hint_bar};

use crate::prefs::{Hotkey, Hotkeys};
use crate::tui::sigil_view::canvas;

use super::rows::row;

const SEPARATOR: &str = " · ";

pub(in crate::tui) const FILTER_HINT: &str = "type to filter";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui) enum Key {
    Enter,
    Esc,
    Tab,
    Space,
    UpDown,
    Hotkey(Hotkey),
    Fixed(&'static str),
}

impl Key {
    #[must_use]
    pub(in crate::tui) fn label(self, keys: &Hotkeys) -> String {
        match self {
            Self::Enter => "↵".to_string(),
            Self::Esc => "esc".to_string(),
            Self::Tab => "tab".to_string(),
            Self::Space => "space".to_string(),
            Self::UpDown => "↑↓".to_string(),
            Self::Hotkey(action) => keys.label(action),
            Self::Fixed(chord) => chord.to_string(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::tui) struct Hints {
    keys: Vec<(Key, String)>,
    right: String,
}

impl Hints {
    #[must_use]
    pub(in crate::tui) fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub(in crate::tui) fn key(mut self, key: Key, action: impl Into<String>) -> Self {
        self.keys.push((key, action.into()));
        self
    }

    #[must_use]
    pub(in crate::tui) fn right(mut self, text: impl Into<String>) -> Self {
        self.right = text.into();
        self
    }

    #[must_use]
    pub(in crate::tui) fn text(&self, keys: &Hotkeys) -> String {
        self.keys
            .iter()
            .map(|(key, action)| format!("{} {action}", key.label(keys)))
            .collect::<Vec<_>>()
            .join(SEPARATOR)
    }

    #[must_use]
    pub(in crate::tui) fn bar(&self, keys: &Hotkeys) -> NodeBuilder {
        hint_bar(self.text(keys), self.right.clone()).layout(
            row(16.0)
                .align_items(AlignItems::Center)
                .gap(canvas::cell().w * 2.0),
        )
    }
}
