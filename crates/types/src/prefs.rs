#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolOutput {
    Compact,
    Full,
    Hidden,
}

impl ToolOutput {
    pub fn text(self, output: &str) -> &str {
        if self == Self::Full {
            return output;
        }
        let line = output
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        let end = line
            .char_indices()
            .nth(120)
            .map_or(line.len(), |(index, _)| index);
        &line[..end]
    }
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Full => "full",
            Self::Hidden => "hidden",
        }
    }
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Compact => Self::Full,
            Self::Full => Self::Hidden,
            Self::Hidden => Self::Compact,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Off,
    Collapsed,
    Expanded,
}

impl Visibility {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Collapsed => "collapsed",
            Self::Expanded => "expanded",
        }
    }
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Off => Self::Collapsed,
            Self::Collapsed => Self::Expanded,
            Self::Expanded => Self::Off,
        }
    }
    #[must_use]
    pub fn parse(value: &str, fallback: Self) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "false" | "hidden" => Self::Off,
            "collapsed" => Self::Collapsed,
            "expanded" | "on" | "true" | "shown" => Self::Expanded,
            _ => fallback,
        }
    }
}
