//! Session and in-turn identities.

use std::fmt;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One session: the directory name under `<home>/sessions/`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SessionId(u128);

impl SessionId {
    /// A fresh id: wall-clock nanoseconds mixed with the process id, so
    /// ids sort by creation time and two processes never collide.
    pub(crate) fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self(nanos ^ u128::from(std::process::id()))
    }

    #[cfg(test)]
    pub(crate) fn from_u128(value: u128) -> Self {
        Self(value)
    }
}

impl fmt::Debug for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:032x}", self.0)
    }
}

impl FromStr for SessionId {
    type Err = std::num::ParseIntError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            // Force a parse error for anything but 32 hex digits.
            return "".parse::<u128>().map(Self);
        }
        u128::from_str_radix(value, 16).map(Self)
    }
}

impl Serialize for SessionId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SessionId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// One provider round inside the open turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Attempt(pub(super) u64);

/// One tool call the provider asked for inside the open turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Call(pub(super) u64);
