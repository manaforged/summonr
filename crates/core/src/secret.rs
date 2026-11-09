use std::fmt::{Debug, Display, Formatter, Result};

use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Debug for Secret {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter.write_str("[secret]")
    }
}

impl Display for Secret {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter.write_str("[secret]")
    }
}
