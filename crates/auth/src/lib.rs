use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use leyline::{RequestBuilder, Session};

mod oauth;

pub use oauth::{Grant, Spec};

pub const AGENT: &str = concat!("summonr/", env!("CARGO_PKG_VERSION"));

const SKEW: Duration = Duration::from_secs(60);

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

pub trait Store: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>>;
    fn set(&self, name: &str, secret: &str) -> Result<()>;
    fn forget(&self, name: &str) -> Result<bool>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    Key(String),
    Login(Spec),
}

struct Seat {
    spec: Spec,
    name: String,
    held: Mutex<Option<Issued>>,
}

#[derive(Clone)]
pub(crate) struct Issued {
    pub(crate) access: String,
    pub(crate) refresh: Option<String>,
    pub(crate) expires: Option<Instant>,
}

impl Issued {
    pub(crate) fn fresh(&self) -> bool {
        self.expires
            .is_none_or(|at| at.saturating_duration_since(Instant::now()) > SKEW)
    }
}

enum Held {
    Key(String),
    Login(Box<Seat>),
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
            Method::Login(spec) => Held::Login(Box::new(Seat {
                spec,
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
            Held::Key(key) => Ok(key.clone()),
            Held::Login(seat) => oauth::token(parts(seat), http, self.store.as_ref()).await,
        }
    }

    pub async fn apply(&self, request: RequestBuilder, http: &Session) -> Result<RequestBuilder> {
        let token = self.token(http).await?;
        Ok(self.scheme.apply(request, &token))
    }

    pub async fn login(&self, http: &Session, notice: impl FnMut(&str)) -> Result<()> {
        let Held::Login(seat) = self.held.as_ref() else {
            return Err(Error::Config(
                "this endpoint takes a key, not a login".to_string(),
            ));
        };
        oauth::login(parts(seat), http, self.store.as_ref(), notice).await
    }

    pub fn forget_live(&self) {
        if let Held::Login(seat) = self.held.as_ref()
            && let Ok(mut slot) = seat.held.lock()
        {
            slot.take();
        }
    }

    pub fn ready(&self) -> Result<bool> {
        match self.held.as_ref() {
            Held::Key(_) => Ok(true),
            Held::Login(seat) => Ok(self
                .store
                .get(&seat.name)?
                .is_some_and(|raw| oauth::parse_stored(&raw).is_some())),
        }
    }
}

pub(crate) struct Parts<'a> {
    pub(crate) spec: &'a Spec,
    pub(crate) name: &'a str,
    pub(crate) held: &'a Mutex<Option<Issued>>,
}

fn parts(seat: &Seat) -> Parts<'_> {
    Parts {
        spec: &seat.spec,
        name: &seat.name,
        held: &seat.held,
    }
}

pub fn http(timeout: Duration) -> Result<Session> {
    Session::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| Error::Transport(error.to_string()))
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
