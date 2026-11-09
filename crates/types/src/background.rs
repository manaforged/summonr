#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Background {
    #[default]
    Theme,
    Terminal,
}

impl Background {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Terminal => "terminal",
        }
    }

    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Theme => Self::Terminal,
            Self::Terminal => Self::Theme,
        }
    }
}
