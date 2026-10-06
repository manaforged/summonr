use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Presence {
    Joined,
    Idle,
    Working,
    Unknown,
}

impl Presence {
    const ALL: [Self; 4] = [Self::Joined, Self::Idle, Self::Working, Self::Unknown];

    pub(crate) const fn wire(self) -> &'static str {
        match self {
            Self::Joined => "joined",
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Unknown => "unknown",
        }
    }

    fn parse(text: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|presence| presence.wire() == text)
            .unwrap_or(Self::Unknown)
    }
}

impl std::fmt::Display for Presence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.wire())
    }
}

impl Serialize for Presence {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.wire())
    }
}

impl<'de> Deserialize<'de> for Presence {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|text| Self::parse(&text))
    }
}
