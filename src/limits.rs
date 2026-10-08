use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;

use crate::endpoint::CacheTtl;

#[cfg(not(test))]
pub(crate) const JOB_WAIT_SECS: u64 = 120;
#[cfg(test)]
pub(crate) const JOB_WAIT_SECS: u64 = 1;
pub(crate) const JOB_WAIT: Duration = Duration::from_secs(JOB_WAIT_SECS);
#[cfg(not(test))]
pub(crate) const COMMAND_WAIT_SECS: u64 = 120;
#[cfg(test)]
pub(crate) const COMMAND_WAIT_SECS: u64 = 1;
#[cfg(not(test))]
pub(crate) const COMMAND_KILL_SECS: u64 = 3600;
#[cfg(test)]
pub(crate) const COMMAND_KILL_SECS: u64 = 4;
pub(crate) const GRANT_WAIT: Duration = Duration::from_secs(600);
pub(crate) const JOB_POLL_START_SECS: u64 = 1;
pub(crate) const JOB_POLL_CAP_SECS: u64 = 2;
pub(crate) const MAIL_STALE_SECS: u64 = 120;
pub(crate) const MAIL_SWEEP_SECS: u64 = 24 * 3600;
pub(crate) const VERIFY_TIMEOUT_SECS: u64 = 600;
pub(crate) const PROBE: Duration = Duration::from_secs(5);
pub(crate) const AUTH: Duration = Duration::from_secs(30);
pub(crate) const MODEL_PROBE: Duration = Duration::from_secs(8);
pub(crate) const WEB_SEARCH: Duration = Duration::from_secs(15);
pub(crate) const WEB_CONNECT: Duration = Duration::from_secs(8);
pub(crate) const WEB_FETCH: Duration = Duration::from_secs(20);
pub(crate) const WEB_BODY: usize = 1024 * 1024;
#[cfg(not(test))]
pub(crate) const KEYCHAIN: Duration = Duration::from_secs(5);
#[cfg(test)]
pub(crate) const KEYCHAIN: Duration = Duration::from_millis(600);
pub(crate) const KEYCHAIN_POLL: Duration = Duration::from_millis(20);
pub(crate) const MCP_OPEN: Duration = Duration::from_secs(60);
pub(crate) const MCP_CALL: Duration = Duration::from_secs(300);
pub(crate) const MCP_IDLE: Duration = Duration::from_secs(120);
pub(crate) const MCP_PROGRESS_STRETCH: u32 = 4;
pub(crate) const MCP_CANCEL_WRITE: Duration = Duration::from_secs(1);
pub(crate) const MCP_LOG_MAX: u64 = 1024 * 1024;
pub(crate) const MCP_FRAME_MAX: usize = 4 * 1024 * 1024;
pub(crate) const MCP_STDERR_TAIL: usize = 2 * 1024;
pub(crate) const MCP_STDERR_SETTLE: Duration = Duration::from_secs(1);
pub(crate) const MCP_DEFER_BYTES: usize = 8 * 1024;
pub(crate) const MCP_FOUND_MAX: usize = 8;
pub(crate) const LSP_START: Duration = Duration::from_secs(30);
pub(crate) const LSP_REQUEST: Duration = Duration::from_secs(30);
pub(crate) const LSP_CHECK: Duration = Duration::from_secs(5);
pub const LOG_RETAIN: Duration = Duration::from_secs(14 * 24 * 60 * 60);
pub const SESSION_RETAIN: Duration = Duration::from_secs(30 * 24 * 60 * 60);
pub const JOB_RETAIN: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub(crate) const SPEED_WINDOW: Duration = Duration::from_secs(3);
pub(crate) const READER_WINDOW: Duration = Duration::from_millis(400);
pub(crate) const FRAME_WINDOW: Duration = Duration::from_secs(1);
pub(crate) const ANIMATION: Duration = Duration::from_millis(40);
pub(crate) const SPINNER: Duration = Duration::from_millis(100);
pub(crate) const SPLASH_REVEAL_MS: u128 = 760;
pub(crate) const STATUS_REFRESH: Duration = Duration::from_secs(1);
pub(crate) const BEAT: Duration = Duration::from_secs(2);
pub(crate) const NOTICE: Duration = Duration::from_secs(2);
#[cfg(feature = "gui")]
pub(crate) const WORKER_STOP: Duration = Duration::from_secs(2);
pub(crate) const REPEAT_WINDOW: Duration = Duration::from_secs(2);
pub(crate) const STEP_ATTEMPTS: u16 = 3;
pub(crate) const REPAIR_LIMIT: usize = 6;
pub(crate) const EDIT_HUNK_ROWS: usize = 80;
pub(crate) const DISPLAY_PATCH_LINES: usize = 1_000;
pub(crate) const TOKEN_EXPIRY_SKEW: Duration = Duration::from_secs(60);
pub(crate) const WEB_REDIRECT_MAX: usize = 5;
pub(crate) const READ_LINE_LIMIT: usize = 1000;

pub(crate) const fn seconds(value: u64) -> Duration {
    Duration::from_secs(value)
}

pub(crate) const PASTE_FOLD_LINES: usize = 8;
pub(crate) const PASTE_FOLD_CHARS: usize = 1000;

const BUILT_IN: &str = include_str!("limits.toml");

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Limits {
    pub cache_ttl: CacheTtl,
    pub retry_limit: u32,
    pub retry_base_ms: u64,
    pub retry_max_ms: u64,
    pub retry_after_max_secs: u64,
    pub stream_idle_secs: u64,
    pub stream_total_secs: u64,
    pub connect_secs: u64,
    pub compact_percent: usize,
    pub compact_chars: usize,
    pub summary_retry_rounds: usize,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Overrides {
    pub cache_ttl: Option<CacheTtl>,
    pub retry_limit: Option<u32>,
    pub retry_base_ms: Option<u64>,
    pub retry_max_ms: Option<u64>,
    pub retry_after_max_secs: Option<u64>,
    pub stream_idle_secs: Option<u64>,
    pub stream_total_secs: Option<u64>,
    pub connect_secs: Option<u64>,
    pub compact_percent: Option<usize>,
    pub compact_chars: Option<usize>,
    pub summary_retry_rounds: Option<usize>,
}

impl Limits {
    pub(crate) fn built_in() -> Self {
        match toml::from_str(BUILT_IN) {
            Ok(limits) => limits,
            Err(error) => panic!("the built-in limits table is invalid: {error}"),
        }
    }

    pub(crate) fn overlay(mut self, with: &Overrides) -> Self {
        self.cache_ttl = with.cache_ttl.unwrap_or(self.cache_ttl);
        self.retry_limit = with.retry_limit.unwrap_or(self.retry_limit);
        self.retry_base_ms = with.retry_base_ms.unwrap_or(self.retry_base_ms);
        self.retry_max_ms = with.retry_max_ms.unwrap_or(self.retry_max_ms);
        self.retry_after_max_secs = with
            .retry_after_max_secs
            .unwrap_or(self.retry_after_max_secs);
        self.stream_idle_secs = with.stream_idle_secs.unwrap_or(self.stream_idle_secs);
        self.stream_total_secs = with.stream_total_secs.unwrap_or(self.stream_total_secs);
        self.connect_secs = with.connect_secs.unwrap_or(self.connect_secs);
        self.compact_percent = with.compact_percent.unwrap_or(self.compact_percent);
        self.compact_chars = with.compact_chars.unwrap_or(self.compact_chars);
        self.summary_retry_rounds = with
            .summary_retry_rounds
            .unwrap_or(self.summary_retry_rounds);
        self
    }

    pub(crate) fn retry_base(&self) -> Duration {
        Duration::from_millis(self.retry_base_ms)
    }

    pub(crate) fn retry_max(&self) -> Duration {
        Duration::from_millis(self.retry_max_ms)
    }

    pub(crate) fn retry_after_max(&self) -> Duration {
        Duration::from_secs(self.retry_after_max_secs)
    }

    pub(crate) fn stream_idle(&self) -> Duration {
        Duration::from_secs(self.stream_idle_secs)
    }

    pub(crate) fn stream_total(&self) -> Duration {
        Duration::from_secs(self.stream_total_secs)
    }

    pub(crate) fn connect(&self) -> Duration {
        Duration::from_secs(self.connect_secs)
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
    Limits::built_in()
}

pub(crate) fn limits() -> &'static Limits {
    static LOADED: OnceLock<Limits> = OnceLock::new();
    LOADED.get_or_init(load)
}

#[cfg(test)]
#[path = "limits_tests.rs"]
mod tests;
