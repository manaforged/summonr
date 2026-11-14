/// How a session started, as a `session_start` hook sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    Startup,
    Resume,
    Fork,
    Compact,
}

impl Start {
    /// The `source` value and matcher subject.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Resume => "resume",
            Self::Fork => "fork",
            Self::Compact => "compact",
        }
    }
}

/// What started a compaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compaction {
    Manual,
    Auto,
}

impl Compaction {
    /// The `trigger` value and matcher subject.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
        }
    }
}

/// Why a session ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Clear,
    Resume,
    Exit,
    Other,
}

impl End {
    /// The `reason` value and matcher subject.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Resume => "resume",
            Self::Exit => "prompt_input_exit",
            Self::Other => "other",
        }
    }
}

/// The permission mode a hook sees: approval prompts on, or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Ask,
    Auto,
}

impl Permission {
    /// The Claude Code `permission_mode` value.
    #[must_use]
    pub const fn claude(self) -> &'static str {
        match self {
            Self::Ask => "default",
            Self::Auto => "auto",
        }
    }
}
