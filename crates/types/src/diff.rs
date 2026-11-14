/// How the diff pane lays out a change: side by side or unified.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffLayout {
    #[default]
    Split,
    Unified,
}

impl DiffLayout {
    /// The name stored in the config file.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Split => "split",
            Self::Unified => "unified",
        }
    }

    /// The other diff layout.
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Split => Self::Unified,
            Self::Unified => Self::Split,
        }
    }
}
