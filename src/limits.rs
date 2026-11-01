use std::sync::OnceLock;
use std::time::Duration;

mod durations;
mod fields;

pub(crate) use fields::{Limits, Overrides};

pub(crate) const fn seconds(value: u64) -> Duration {
    Duration::from_secs(value)
}

const BUILT_IN: &str = include_str!("limits.toml");
#[cfg(test)]
const TEST: &str = include_str!("limits_test.toml");
#[cfg(test)]
const TEST_WINDOWS: &str = include_str!("limits_test_windows.toml");

impl Limits {
    pub(crate) fn built_in() -> Self {
        summonr_core::table::embedded("limits", BUILT_IN)
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

    pub(crate) fn retry_wait(&self, retry_after: Option<u64>, retries: u32) -> Duration {
        retry_after.map_or_else(
            || self.retry_policy(Reach::Remote).delay(retries),
            |secs| Duration::from_secs(secs).min(self.retry_after_max()),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reach {
    Remote,
    Local,
}

#[cfg(not(test))]
fn load() -> Limits {
    let built_in = Limits::built_in();
    match crate::config::path().and_then(|path| crate::config::parse(&path)) {
        Ok(file) => match &file.limits {
            Some(with) => built_in.overlay(with),
            None => built_in,
        },
        Err(_) => built_in,
    }
}

#[cfg(test)]
fn load() -> Limits {
    let test = Limits::built_in().overlay(&summonr_core::table::embedded("test limits", TEST));
    if cfg!(windows) {
        test.overlay(&summonr_core::table::embedded(
            "windows test limits",
            TEST_WINDOWS,
        ))
    } else {
        test
    }
}

pub(crate) fn limits() -> &'static Limits {
    static LOADED: OnceLock<Limits> = OnceLock::new();
    LOADED.get_or_init(load)
}

pub fn log_retain() -> Duration {
    limits().log_retain()
}

#[cfg(test)]
#[path = "limits_tests.rs"]
mod tests;
