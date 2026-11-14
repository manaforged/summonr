use std::fmt;
use std::time::Duration;

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
    kind: ProviderFailureKind,
    status: Option<u16>,
    message: String,
    partial_output: bool,
    retry_after: Option<u64>,
}

impl ProviderFailure {
    /// A failure of `kind` with no status, no partial output, and no retry hint.
    pub fn new(kind: ProviderFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            status: None,
            message: message.into(),
            partial_output: false,
            retry_after: None,
        }
    }

    /// The class of the failure.
    pub const fn kind(&self) -> ProviderFailureKind {
        self.kind
    }

    /// The HTTP status the provider answered with, if any.
    pub const fn status(&self) -> Option<u16> {
        self.status
    }

    /// The text shown for the failure.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether the provider sent output before it failed.
    pub const fn partial_output(&self) -> bool {
        self.partial_output
    }

    /// The seconds the provider asked to wait before a retry, if it asked.
    pub const fn retry_after(&self) -> Option<u64> {
        self.retry_after
    }

    /// Sets the class of the failure.
    pub const fn set_kind(&mut self, kind: ProviderFailureKind) {
        self.kind = kind;
    }

    /// Sets the HTTP status.
    pub const fn set_status(&mut self, status: Option<u16>) {
        self.status = status;
    }

    /// Sets the text shown for the failure.
    pub fn set_message(&mut self, message: impl Into<String>) {
        self.message = message.into();
    }

    /// Sets whether the provider sent output before it failed.
    pub const fn set_partial_output(&mut self, partial_output: bool) {
        self.partial_output = partial_output;
    }

    /// Sets the seconds to wait before a retry.
    pub const fn set_retry_after(&mut self, retry_after: Option<u64>) {
        self.retry_after = retry_after;
    }
}

impl fmt::Display for ProviderFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// The class of an error, which callers match instead of the variant.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Config,
    Held,
    Setup,
    Io,
    Json,
    Provider,
    Tool,
    Denied,
    Timeout,
}

impl ErrorKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Config => "configuration error: ",
            Self::Io => "I/O error: ",
            Self::Json => "JSON error: ",
            Self::Provider => "provider error: ",
            Self::Tool => "tool error: ",
            Self::Denied => "policy denied: ",
            Self::Held | Self::Setup | Self::Timeout => "",
        }
    }
}

const HELD: &str = "This chat is open in another Summonr window.";

/// The one error type of the harness.
#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    Config(String),
    Held,
    Setup(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Provider(ProviderFailure),
    Tool(String),
    Denied(String),
    Timeout {
        what: String,
        limit: Duration,
    },
    Caused {
        kind: ErrorKind,
        message: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl Error {
    /// A provider error of `kind` with no status and no retry hint.
    pub fn provider(kind: ProviderFailureKind, message: impl Into<String>) -> Self {
        Self::Provider(ProviderFailure::new(kind, message))
    }

    /// An error of `kind` whose text is `source`'s, with `source` kept as `source()`.
    pub fn caused(kind: ErrorKind, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Caused {
            kind,
            message: source.to_string(),
            source: Box::new(source),
        }
    }

    /// This error's detail under the label of `kind`, with this error kept as `source()`.
    #[must_use]
    pub fn relabel(self, kind: ErrorKind) -> Self {
        Self::Caused {
            kind,
            message: self.detail(),
            source: Box::new(self),
        }
    }

    /// The class of this error.
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Config(_) => ErrorKind::Config,
            Self::Held => ErrorKind::Held,
            Self::Setup(_) => ErrorKind::Setup,
            Self::Io(_) => ErrorKind::Io,
            Self::Json(_) => ErrorKind::Json,
            Self::Provider(_) => ErrorKind::Provider,
            Self::Tool(_) => ErrorKind::Tool,
            Self::Denied(_) => ErrorKind::Denied,
            Self::Timeout { .. } => ErrorKind::Timeout,
            Self::Caused { kind, .. } => *kind,
        }
    }

    /// The text without the label of its kind.
    pub fn detail(&self) -> String {
        match self {
            Self::Config(text)
            | Self::Setup(text)
            | Self::Tool(text)
            | Self::Denied(text)
            | Self::Caused { message: text, .. } => text.clone(),
            Self::Held => HELD.to_string(),
            Self::Io(error) => error.to_string(),
            Self::Json(error) => error.to_string(),
            Self::Provider(failure) => failure.message.clone(),
            Self::Timeout { what, limit } => format!("{what} ran past {}s", limit.as_secs()),
        }
    }

    /// The provider failure inside this error, if it is one.
    pub const fn provider_failure(&self) -> Option<&ProviderFailure> {
        match self {
            Self::Provider(failure) => Some(failure),
            Self::Config(_)
            | Self::Held
            | Self::Setup(_)
            | Self::Io(_)
            | Self::Json(_)
            | Self::Tool(_)
            | Self::Denied(_)
            | Self::Timeout { .. }
            | Self::Caused { .. } => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.kind().label())?;
        formatter.write_str(&self.detail())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Caused { source, .. } => Some(source.as_ref()),
            Self::Config(_)
            | Self::Held
            | Self::Setup(_)
            | Self::Provider(_)
            | Self::Tool(_)
            | Self::Denied(_)
            | Self::Timeout { .. } => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// A result with the harness error type.
pub type Result<T> = std::result::Result<T, Error>;
