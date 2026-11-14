use serde::{Deserialize, Serialize};

/// What a child agent may do: implement edits or research only.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    #[default]
    Implement,
    Research,
}

impl Access {
    /// The name the task tool takes.
    pub fn name(self) -> &'static str {
        match self {
            Self::Implement => "implement",
            Self::Research => "research",
        }
    }
}

/// Which child agents an action covers: all, or only the ones not in the background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildScope {
    All,
    Foreground,
}
