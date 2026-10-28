use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    #[default]
    Implement,
    Research,
}

impl Access {
    pub fn name(self) -> &'static str {
        match self {
            Self::Implement => "implement",
            Self::Research => "research",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildScope {
    All,
    Foreground,
}
