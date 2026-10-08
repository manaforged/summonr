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

fn parse<T: serde::de::DeserializeOwned>(text: &str, name: &str) -> T {
    match toml::from_str(text) {
        Ok(table) => table,
        Err(error) => panic!("the {name} limits table is invalid: {error}"),
    }
}

impl Limits {
    pub(crate) fn built_in() -> Self {
        parse(BUILT_IN, "built-in")
    }

    pub(crate) fn retry_policy(&self) -> leyline::RetryPolicy {
        leyline::RetryPolicy::transient()
            .max_retries(self.retry_limit)
            .initial_backoff(self.retry_base())
            .max_backoff(self.retry_max())
            .max_retry_after(self.retry_after_max())
            .retry_on([
                leyline::RetryTrigger::ConnectionError,
                leyline::RetryTrigger::Status(429),
                leyline::RetryTrigger::ServerError,
            ])
    }
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
    Limits::built_in().overlay(&parse(TEST, "test"))
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
