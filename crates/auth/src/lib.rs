use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use leyline::{RequestBuilder, Session};
use summonr_core::error::{Error, ErrorKind, ProviderFailureKind, Result};
use summonr_core::secret::Secret;

mod oauth;
/// Opens a URL in the user's browser.
pub mod open;

pub use oauth::{Grant, Spec, TokenStyle};

/// The `User-Agent` value: `summonr/` and the crate version.
pub const AGENT: &str = concat!("summonr/", env!("CARGO_PKG_VERSION"));

struct Waits {
    skew: Duration,
    login: Duration,
    poll_floor: Duration,
    poll_ceiling: Duration,
    poll_step: Duration,
    device_expiry: Duration,
}

const WAITS: Waits = Waits {
    skew: Duration::from_secs(60),
    login: Duration::from_secs(300),
    poll_floor: Duration::from_secs(1),
    poll_ceiling: Duration::from_secs(30),
    poll_step: Duration::from_secs(5),
    device_expiry: Duration::from_secs(900),
};

const TIMED_OUT: &str = "the login timed out";

fn refused(text: impl Into<String>) -> Error {
    Error::provider(ProviderFailureKind::Authentication, text)
}

fn unreachable(error: impl std::fmt::Display) -> Error {
    Error::provider(ProviderFailureKind::Transport, error.to_string())
}

fn needs_login(name: &str, detail: &str) -> Error {
    refused(format!("{name} needs a login: {detail}"))
}

/// The future that resolves to the text a person pasted back, if any.
pub type Answer = std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send>>;

/// How a sign-in shows lines to a person and asks for pasted text.
pub struct Prompt {
    say: Box<dyn FnMut(&str) + Send>,
    ask: Box<dyn FnMut(String) -> Answer + Send>,
    link: Option<Box<dyn FnMut(&str, &str) + Send>>,
}

impl Prompt {
    /// A prompt that shows lines with `say` and asks with `ask`.
    pub fn new(
        say: impl FnMut(&str) + Send + 'static,
        ask: impl FnMut(String) -> Answer + Send + 'static,
    ) -> Self {
        Self {
            say: Box::new(say),
            ask: Box::new(ask),
            link: None,
        }
    }

    /// Shows sign-in links with `link(url, code)` instead of as a line of text.
    #[must_use]
    pub fn with_link(mut self, link: impl FnMut(&str, &str) + Send + 'static) -> Self {
        self.link = Some(Box::new(link));
        self
    }

    /// A prompt that shows lines and never gets an answer.
    pub fn telling(say: impl FnMut(&str) + Send + 'static) -> Self {
        Self::new(say, |_| Box::pin(std::future::ready(None)))
    }

    /// Shows `line` to the person.
    pub fn notice(&mut self, line: &str) {
        (self.say)(line);
    }

    /// Shows the sign-in `url` and the `code` to enter there, which may be empty.
    pub fn open(&mut self, url: &str, code: &str) {
        match &mut self.link {
            Some(link) => link(url, code),
            None if code.is_empty() => (self.say)(&format!("open {url}")),
            None => (self.say)(&format!("open {url} and enter {code}")),
        }
    }

    /// Asks with `line` and returns the trimmed answer, or fails when none came.
    pub async fn request(&mut self, line: &str) -> Result<String> {
        (self.ask)(line.to_string())
            .await
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .ok_or_else(|| {
                Error::Config("this sign-in needs the code pasted back, and none came".to_string())
            })
    }
}

/// Where keys and login grants are kept by name.
pub trait Store: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>>;
    fn set(&self, name: &str, secret: &str) -> Result<()>;
    fn forget(&self, name: &str) -> Result<bool>;
    fn claim(&self, _name: &str) -> Result<Box<dyn Send>> {
        Ok(Box::new(()))
    }
}

/// How a token goes on a request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scheme {
    #[default]
    Bearer,
    ApiKey,
    GoogleKey,
}

impl Scheme {
    /// Puts `token` on `request` as this scheme's header.
    pub fn apply(self, request: RequestBuilder, token: &str) -> RequestBuilder {
        match self {
            Self::Bearer => request.bearer_auth(token),
            Self::ApiKey => request.header("x-api-key", token),
            Self::GoogleKey => request.header("x-goog-api-key", token),
        }
    }
}

impl From<summonr_types::oauth::TokenScheme> for Scheme {
    fn from(scheme: summonr_types::oauth::TokenScheme) -> Self {
        match scheme {
            summonr_types::oauth::TokenScheme::Bearer => Self::Bearer,
            summonr_types::oauth::TokenScheme::ApiKey => Self::ApiKey,
            summonr_types::oauth::TokenScheme::GoogleKey => Self::GoogleKey,
        }
    }
}

/// A fixed key or an OAuth login.
#[derive(Clone, Debug)]
pub enum Method {
    Key(Secret),
    Login(Box<Spec>),
}

struct LoginState {
    spec: Spec,
    name: String,
    held: Mutex<Option<Issued>>,
    renewing: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub(crate) struct Issued {
    pub(crate) access: String,
    pub(crate) refresh: Option<String>,
    pub(crate) expires: Option<Instant>,
    pub(crate) identity: Option<String>,
    pub(crate) client: Option<String>,
}

impl Issued {
    pub(crate) fn fresh(&self) -> bool {
        self.expires
            .is_none_or(|at| at.saturating_duration_since(Instant::now()) > WAITS.skew)
    }
}

enum Held {
    Key(Secret),
    Login(Box<LoginState>),
}

/// The credential of one endpoint, shared across its requests.
#[derive(Clone)]
pub struct Auth {
    held: Arc<Held>,
    scheme: Scheme,
    store: Arc<dyn Store>,
}

impl Auth {
    /// The credential `name` with its method, scheme, and store.
    pub fn new(method: Method, scheme: Scheme, name: &str, store: Arc<dyn Store>) -> Self {
        let held = match method {
            Method::Key(key) => Held::Key(key),
            Method::Login(spec) => Held::Login(Box::new(LoginState {
                spec: *spec,
                name: name.to_string(),
                held: Mutex::new(None),
                renewing: tokio::sync::Mutex::new(()),
            })),
        };
        Self {
            held: Arc::new(held),
            scheme,
            store,
        }
    }

    /// Whether this credential is an OAuth login.
    pub fn is_login(&self) -> bool {
        matches!(self.held.as_ref(), Held::Login(_))
    }

    /// How the token goes on a request.
    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// The current token, refreshed when a login token is stale.
    pub async fn token(&self, http: &Session) -> Result<String> {
        match self.held.as_ref() {
            Held::Key(key) => Ok(key.expose().to_string()),
            Held::Login(login) => oauth::token(parts(login), http, &self.store).await,
        }
    }

    /// Puts the current token on `request`.
    pub async fn apply(&self, request: RequestBuilder, http: &Session) -> Result<RequestBuilder> {
        let token = self.token(http).await?;
        Ok(self.scheme.apply(request, &token))
    }

    /// Runs the sign-in flow; fails for a key credential.
    pub async fn login(&self, http: &Session, prompt: &mut Prompt) -> Result<()> {
        let Held::Login(login) = self.held.as_ref() else {
            return Err(Error::Config(
                "this endpoint takes a key, not a login".to_string(),
            ));
        };
        oauth::login(parts(login), http, &self.store, prompt).await
    }

    /// The headers built from claims in the held login token.
    pub fn stamped(&self) -> Vec<(String, String)> {
        let Held::Login(login) = self.held.as_ref() else {
            return Vec::new();
        };
        if login.spec.claim_headers.is_empty() {
            return Vec::new();
        }
        let Ok(slot) = login.held.lock() else {
            return Vec::new();
        };
        let Some(issued) = slot.as_ref() else {
            return Vec::new();
        };
        oauth::stamped(&login.spec, issued)
    }

    /// Drops the held login token and marks a stored grant with no exchange as expired.
    pub fn forget_live(&self) {
        let Held::Login(login) = self.held.as_ref() else {
            return;
        };
        if let Ok(mut slot) = login.held.lock() {
            slot.take();
        }
        if login.spec.exchange().is_none() {
            drop(oauth::stale(&parts(login), self.store.as_ref()));
        }
    }

    /// Whether a key is set or a readable login grant is stored.
    pub fn ready(&self) -> Result<bool> {
        match self.held.as_ref() {
            Held::Key(_) => Ok(true),
            Held::Login(login) => Ok(self
                .store
                .get(&login.name)?
                .is_some_and(|raw| oauth::parse_stored(&raw).is_some())),
        }
    }
}

pub(crate) struct Parts<'a> {
    pub(crate) spec: &'a Spec,
    pub(crate) name: &'a str,
    pub(crate) held: &'a Mutex<Option<Issued>>,
    pub(crate) renewing: &'a tokio::sync::Mutex<()>,
}

fn parts(login: &LoginState) -> Parts<'_> {
    Parts {
        spec: &login.spec,
        name: &login.name,
        held: &login.held,
        renewing: &login.renewing,
    }
}

/// An HTTP session with these total and connect timeouts.
pub fn http(timeout: Duration, connect: Duration) -> Result<Session> {
    builder(timeout, connect)
        .build()
        .map_err(|error| Error::caused(ErrorKind::Setup, error))
}

/// A session builder with these total and connect timeouts.
pub fn builder(timeout: Duration, connect: Duration) -> leyline::SessionBuilder {
    session(leyline::Timeout::new().total(timeout).connect(connect))
}

/// Starts a session that follows a redirect only when it stays on the same origin.
pub fn session(timeouts: leyline::Timeout) -> leyline::SessionBuilder {
    Session::builder()
        .timeout(timeouts)
        .redirect(leyline::Redirect::custom(|attempt| {
            attempt
                .url()
                .zip(attempt.location())
                .is_some_and(|(from, to)| {
                    from.join(to).is_ok_and(|to| to.origin() == from.origin())
                })
        }))
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "registration_tests.rs"]
mod registration_tests;
