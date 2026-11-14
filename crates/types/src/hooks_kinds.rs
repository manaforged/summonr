use summonr_core::error::{ProviderFailure, ProviderFailureKind};

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

/// What a `notification` hook announces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    PermissionPrompt,
    ElicitationDialog,
}

impl Notice {
    /// The `notification_type` value and matcher subject.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PermissionPrompt => "permission_prompt",
            Self::ElicitationDialog => "elicitation_dialog",
        }
    }
}

/// The provider error type a `stop_failure` hook sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    RateLimit,
    Overloaded,
    AuthenticationFailed,
    InvalidRequest,
    ModelNotFound,
    ServerError,
    Unknown,
}

impl Failure {
    /// The error type of a provider failure.
    #[must_use]
    pub const fn of(failure: &ProviderFailure) -> Self {
        match failure.kind() {
            ProviderFailureKind::Authentication | ProviderFailureKind::MissingKey => {
                Self::AuthenticationFailed
            }
            ProviderFailureKind::RateLimited | ProviderFailureKind::UsageLimit => Self::RateLimit,
            ProviderFailureKind::ContextOverflow => Self::InvalidRequest,
            ProviderFailureKind::Server => match failure.status() {
                Some(529) => Self::Overloaded,
                Some(_) | None => Self::ServerError,
            },
            ProviderFailureKind::Http => Self::of_status(failure.status()),
            ProviderFailureKind::Transport
            | ProviderFailureKind::MalformedToolCall
            | ProviderFailureKind::Malformed
            | ProviderFailureKind::Empty => Self::Unknown,
        }
    }

    const fn of_status(status: Option<u16>) -> Self {
        match status {
            Some(400 | 413) => Self::InvalidRequest,
            Some(401 | 403) => Self::AuthenticationFailed,
            Some(404) => Self::ModelNotFound,
            Some(429) => Self::RateLimit,
            Some(529) => Self::Overloaded,
            Some(500..=599) => Self::ServerError,
            Some(_) | None => Self::Unknown,
        }
    }

    /// The `error` value and matcher subject.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::RateLimit => "rate_limit",
            Self::Overloaded => "overloaded",
            Self::AuthenticationFailed => "authentication_failed",
            Self::InvalidRequest => "invalid_request",
            Self::ModelNotFound => "model_not_found",
            Self::ServerError => "server_error",
            Self::Unknown => "unknown",
        }
    }
}
