use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderFailureKind {
    Authentication,
    RateLimited,
    Server,
    Transport,
    ContextOverflow,
    MalformedToolCall,
    Malformed,
    Http,
}

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

#[derive(Debug, Error)]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(String),
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
}

impl Error {
    pub fn provider(kind: ProviderFailureKind, message: impl Into<String>) -> Self {
        Self::Provider(ProviderFailure {
            kind,
            status: None,
            message: message.into(),
            partial_output: false,
            retry_after: None,
        })
    }

    pub(crate) fn provider_failure(&self) -> Option<&ProviderFailure> {
        match self {
            Self::Provider(failure) => Some(failure),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
