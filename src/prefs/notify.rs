#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Notify {
    Off,
    #[default]
    Turn,
    All,
}

impl Notify {
    #[must_use]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Turn => "turn",
            Self::All => "all",
        }
    }

    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Turn => "when a turn ends",
            Self::All => "turn end and approvals",
        }
    }

    #[must_use]
    pub(crate) fn toggle(self) -> Self {
        match self {
            Self::Off => Self::Turn,
            Self::Turn => Self::All,
            Self::All => Self::Off,
        }
    }

    #[must_use]
    pub(crate) fn turns(self) -> bool {
        self != Self::Off
    }

    #[must_use]
    pub(crate) fn approvals(self) -> bool {
        self == Self::All
    }
}
