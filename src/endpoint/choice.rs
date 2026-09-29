#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Config,
    Model,
    Key,
    Seat,
}

impl Need {
    pub(super) const fn key(self) -> bool {
        matches!(self, Self::Key | Self::Seat)
    }

    pub(super) const fn model(self) -> bool {
        matches!(self, Self::Model | Self::Seat)
    }

    pub(crate) const fn without_key(self) -> Self {
        match self {
            Self::Key => Self::Config,
            Self::Seat => Self::Model,
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Flag,
    ModelFlag,
    Env,
    LastUsed,
    Default,
}

impl std::fmt::Display for Pick {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Flag => "flag",
            Self::ModelFlag => "--model",
            Self::Env => "env",
            Self::LastUsed => "last used",
            Self::Default => "default",
        })
    }
}
