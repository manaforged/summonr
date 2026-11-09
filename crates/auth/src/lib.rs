use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use leyline::{RequestBuilder, Session};
use summonr_core::secret::Secret;

mod oauth;
pub mod open;

pub use oauth::{Grant, Spec, TokenStyle};

pub const AGENT: &str = concat!("summonr/", env!("CARGO_PKG_VERSION"));

const SKEW: Duration = Duration::from_secs(60);
const LOGIN_WAIT: Duration = Duration::from_secs(300);
const POLL_FLOOR: Duration = Duration::from_secs(1);
const POLL_CEILING: Duration = Duration::from_secs(30);
const POLL_STEP: Duration = Duration::from_secs(5);
const DEVICE_EXPIRY: Duration = Duration::from_secs(900);

const fn seconds(value: u64) -> Duration {
    Duration::from_secs(value)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Denied(String),
    #[error("{0}")]
    Transport(String),
    #[error("{name} needs a login: {detail}")]
    NeedsLogin { name: String, detail: String },
}

pub type Result<T> = std::result::Result<T, Error>;

pub type Answer = std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send>>;

pub struct Prompt {
    say: Box<dyn FnMut(&str) + Send>,
    ask: Box<dyn FnMut(String) -> Answer + Send>,
}

impl Prompt {
    pub fn new(
        say: impl FnMut(&str) + Send + 'static,
        ask: impl FnMut(String) -> Answer + Send + 'static,
    ) -> Self {
        Self {
            say: Box::new(say),
            ask: Box::new(ask),
        }
    }

    pub fn telling(say: impl FnMut(&str) + Send + 'static) -> Self {
        Self::new(say, |_| Box::pin(std::future::ready(None)))
    }

    pub fn notice(&mut self, line: &str) {
        (self.say)(line);
    }

    pub async fn request(&mut self, line: &str) -> Result<String> {
        (self.ask)(line.to_string())
            .await
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .ok_or_else(|| {
                Error::Denied("this sign-in needs the code pasted back, and none came".to_string())
            })
    }
}

pub trait Store: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>>;
    fn set(&self, name: &str, secret: &str) -> Result<()>;
    fn forget(&self, name: &str) -> Result<bool>;
    fn claim(&self, _name: &str) -> Result<Box<dyn Send>> {
        Ok(Box::new(()))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scheme {
    #[default]
    Bearer,
    ApiKey,
    GoogleKey,
}

impl Scheme {
    pub fn apply(self, request: RequestBuilder, token: &str) -> RequestBuilder {
        match self {
            Self::Bearer => request.bearer_auth(token),
            Self::ApiKey => request.header("x-api-key", token),
            Self::GoogleKey => request.header("x-goog-api-key", token),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Method {
    Key(Secret),
    Login(Box<Spec>),
}

struct LoginState {
    spec: Spec,
    name: String,
    held: Mutex<Option<Issued>>,
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
            .is_none_or(|at| at.saturating_duration_since(Instant::now()) > SKEW)
    }
}

enum Held {
    Key(Secret),
    Login(Box<LoginState>),
}

#[derive(Clone)]
pub struct Auth {
    held: Arc<Held>,
    scheme: Scheme,
    store: Arc<dyn Store>,
}

impl Auth {
    pub fn new(method: Method, scheme: Scheme, name: &str, store: Arc<dyn Store>) -> Self {
        let held = match method {
            Method::Key(key) => Held::Key(key),
            Method::Login(spec) => Held::Login(Box::new(LoginState {
                spec: *spec,
                name: name.to_string(),
                held: Mutex::new(None),
            })),
        };
        Self {
            held: Arc::new(held),
            scheme,
            store,
        }
    }

    pub fn is_login(&self) -> bool {
        matches!(self.held.as_ref(), Held::Login(_))
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    pub async fn token(&self, http: &Session) -> Result<String> {
        match self.held.as_ref() {
            Held::Key(key) => Ok(key.expose().to_string()),
            Held::Login(login) => oauth::token(parts(login), http, self.store.as_ref()).await,
        }
    }

    pub async fn apply(&self, request: RequestBuilder, http: &Session) -> Result<RequestBuilder> {
        let token = self.token(http).await?;
        Ok(self.scheme.apply(request, &token))
    }

    pub async fn login(&self, http: &Session, prompt: &mut Prompt) -> Result<()> {
        let Held::Login(login) = self.held.as_ref() else {
            return Err(Error::Config(
                "this endpoint takes a key, not a login".to_string(),
            ));
        };
        oauth::login(parts(login), http, self.store.as_ref(), prompt).await
    }

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

    pub fn forget_live(&self) {
        let Held::Login(login) = self.held.as_ref() else {
            return;
        };
        if let Ok(mut slot) = login.held.lock() {
            slot.take();
        }
        drop(oauth::stale(&parts(login), self.store.as_ref()));
    }

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
}

fn parts(login: &LoginState) -> Parts<'_> {
    Parts {
        spec: &login.spec,
        name: &login.name,
        held: &login.held,
    }
}

pub fn http(timeout: Duration, connect: Duration) -> Result<Session> {
    builder(timeout, connect)
        .build()
        .map_err(|error| Error::Transport(error.to_string()))
}

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
