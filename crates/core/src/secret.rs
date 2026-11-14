use std::fmt::{Debug, Display, Formatter, Result};

use serde::{Deserialize, Serialize};

/// A secret value that prints as `[secret]` in `Debug` and `Display`.
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Wraps `value` as a secret.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret value itself.
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
