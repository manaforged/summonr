use std::time::Duration;

use thiserror::Error;

/// The class of a provider failure, which decides retry and advice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderFailureKind {
    Authentication,
    MissingKey,
    RateLimited,
    UsageLimit,
    Server,
    Transport,
    ContextOverflow,
    MalformedToolCall,
    Malformed,
    Empty,
    Http,
}

/// A failed provider request with its class, HTTP status, and retry hint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderFailure {
    pub kind: ProviderFailureKind,
    pub status: Option<u16>,
    pub message: String,
    pub partial_output: bool,
    pub retry_after: Option<u64>,
}

impl std::fmt::Display for ProviderFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// The one error type of the harness.
#[derive(Debug, Error)]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("This chat is open in another Summonr window.")]
    Held,
    #[error("{0}")]
    Setup(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("provider error: {0}")]
    Provider(ProviderFailure),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("policy denied: {0}")]
    Denied(String),
    #[error("{what} ran past {}s", .limit.as_secs())]
    Timeout { what: String, limit: Duration },
}

impl Error {
    /// A provider error of `kind` with no status and no retry hint.
    pub fn provider(kind: ProviderFailureKind, message: impl Into<String>) -> Self {
        Self::Provider(ProviderFailure {
            kind,
            status: None,
            message: message.into(),
            partial_output: false,
            retry_after: None,
        })
    }

    /// The provider failure inside this error, if it is one.
    pub fn provider_failure(&self) -> Option<&ProviderFailure> {
        match self {
            Self::Provider(failure) => Some(failure),
            Self::Config(_)
            | Self::Held
            | Self::Setup(_)
            | Self::Io(_)
            | Self::Json(_)
            | Self::Tool(_)
            | Self::Denied(_)
            | Self::Timeout { .. } => None,
        }
    }
}

/// A result with the harness error type.
pub type Result<T> = std::result::Result<T, Error>;
