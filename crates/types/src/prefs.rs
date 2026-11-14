/// How much of a tool's output the transcript shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolOutput {
    Compact,
    Full,
    Hidden,
}

impl ToolOutput {
    /// The part of `output` shown: all of it, or its last non-blank line cut to 120 characters.
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
    /// The name stored in the config file.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Full => "full",
            Self::Hidden => "hidden",
        }
    }
    /// The next tool output setting.
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Compact => Self::Full,
            Self::Full => Self::Hidden,
            Self::Hidden => Self::Compact,
        }
    }
}

/// Whether a section is off, collapsed, or expanded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Off,
    Collapsed,
    Expanded,
}

impl Visibility {
    /// The name stored in the config file.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Collapsed => "collapsed",
            Self::Expanded => "expanded",
        }
    }
    /// The next visibility setting.
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Off => Self::Collapsed,
            Self::Collapsed => Self::Expanded,
            Self::Expanded => Self::Off,
        }
    }
    /// The visibility named by `value`, or `fallback`.
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
