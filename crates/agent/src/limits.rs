use std::sync::OnceLock;
use std::time::Duration;

use summonr_core::error::{Error, Result};

mod fields;

pub(crate) use fields::{Limits, Overrides};

/// Runs `work` for at most `limit`, then fails with `Error::Timeout` naming `what`.
pub(crate) async fn within<T>(
    limit: Duration,
    what: impl Into<String>,
    work: impl Future<Output = T>,
) -> Result<T> {
    tokio::time::timeout(limit, work)
        .await
        .map_err(|_elapsed| Error::Timeout {
            what: what.into(),
            limit,
        })
}

impl Limits {
    pub(crate) fn built_in() -> Self {
        summonr_core::embedded!("limits", "limits.toml")
    }

    pub(crate) fn retry_policy(&self, reach: Reach) -> leyline::Retry {
        self.retry_policy_for(reach, true)
    }

    pub(crate) fn retry_policy_for(&self, reach: Reach, rate_limits: bool) -> leyline::Retry {
        let remote = reach == Reach::Remote;
        leyline::Retry::new(self.retry_limit)
            .backoff(self.retry_base(), self.retry_max())
            .max_wait(self.retry_after_max())
            .when(move |attempt| match (attempt.status(), attempt.error()) {
                (Some(status), _) => {
                    status.is_server_error() || (rate_limits && status.as_u16() == 429)
                }
                (None, Some(error)) => {
                    remote && error.is_retryable() && error.kind() != leyline::Kind::Timeout
                }
                (None, None) => false,
            })
    }

    pub(crate) fn retry_wait(
        &self,
        retry_after: Option<u64>,
        retries: u32,
        asked: Asked,
    ) -> Duration {
        let backoff = self.retry_policy(Reach::Remote).delay(retries);
        match (retry_after.map(Duration::from_secs), asked) {
            (None, _) => backoff,
            (Some(wait), Asked::Capped) => wait.min(self.retry_after_max()),
            (Some(wait), Asked::Whole) => wait.max(backoff),
        }
    }
}

/// How a retry honors the server's Retry-After.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    Capped,
    Whole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reach {
    Remote,
    Local,
}

#[cfg(not(test))]
fn load() -> Limits {
    let built_in = Limits::built_in();
    match crate::config::file() {
        Ok(file) => match &file.limits {
            Some(with) => built_in.overlay(with),
            None => built_in,
        },
        Err(_) => built_in,
    }
}

#[cfg(test)]
fn load() -> Limits {
    let test =
        Limits::built_in().overlay(&summonr_core::embedded!("test limits", "limits_test.toml"));
    if cfg!(windows) {
        test.overlay(&summonr_core::embedded!(
            "windows test limits",
            "limits_test_windows.toml"
        ))
    } else {
        test
    }
}

static LOADED: OnceLock<Limits> = OnceLock::new();

pub fn limits() -> &'static Limits {
    LOADED.get_or_init(load)
}

pub(crate) fn stamp_settle() -> Duration {
    LOADED
        .get()
        .map_or_else(|| Limits::built_in().stamp_settle(), Limits::stamp_settle)
}

pub fn log_retain() -> Duration {
    limits().log_retain()
}

#[cfg(test)]
#[path = "limits_tests.rs"]
mod tests;
