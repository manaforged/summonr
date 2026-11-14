/// When summonr sends a notification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Notify {
    Off,
    #[default]
    Turn,
    All,
}

impl Notify {
    /// The name stored in the config file.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Turn => "turn",
            Self::All => "all",
        }
    }

    /// The setting's display label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Turn => "when a turn ends",
            Self::All => "turn end and approvals",
        }
    }

    /// The next notification setting.
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Off => Self::Turn,
            Self::Turn => Self::All,
            Self::All => Self::Off,
        }
    }

    /// Whether a turn end notifies.
    #[must_use]
    pub fn turns(self) -> bool {
        self != Self::Off
    }

    /// Whether an approval request notifies.
    #[must_use]
    pub fn approvals(self) -> bool {
        self == Self::All
    }
}
