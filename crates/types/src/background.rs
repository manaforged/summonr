/// Where the window background color comes from: the theme or the terminal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Background {
    Theme,
    #[default]
    Terminal,
}

impl Background {
    /// The name stored in the config file.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Terminal => "terminal",
        }
    }

    /// The other background source.
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Theme => Self::Terminal,
            Self::Terminal => Self::Theme,
        }
    }
}
