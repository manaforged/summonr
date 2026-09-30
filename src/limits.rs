use std::time::Duration;

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
pub(crate) const CHILD_POLL: Duration = Duration::from_secs(1);
pub(crate) const GRANT_WAIT: Duration = Duration::from_secs(600);
pub(crate) const GRANT_POLL: Duration = Duration::from_millis(60);
pub(crate) const STREAM_TOTAL: Duration = Duration::from_secs(24 * 60 * 60);
pub(crate) const STREAM_IDLE: Duration = Duration::from_secs(240);
pub(crate) const CONNECT: Duration = Duration::from_secs(20);
pub(crate) const PROBE: Duration = Duration::from_secs(5);
pub(crate) const AUTH: Duration = Duration::from_secs(30);
pub(crate) const MODEL_PROBE: Duration = Duration::from_secs(8);
pub(crate) const WEB_SEARCH: Duration = Duration::from_secs(15);
pub(crate) const WEB_CONNECT: Duration = Duration::from_secs(8);
pub(crate) const WEB_FETCH: Duration = Duration::from_secs(20);
#[cfg(not(test))]
pub(crate) const KEYCHAIN: Duration = Duration::from_secs(5);
#[cfg(test)]
pub(crate) const KEYCHAIN: Duration = Duration::from_millis(600);
pub(crate) const KEYCHAIN_POLL: Duration = Duration::from_millis(20);
pub(crate) const MCP_OPEN: Duration = Duration::from_secs(60);
pub(crate) const MCP_CALL: Duration = Duration::from_secs(300);
pub(crate) const MCP_IDLE: Duration = Duration::from_secs(120);
pub(crate) const MCP_PROGRESS_STRETCH: u32 = 4;
pub(crate) const MCP_LOG_MAX: u64 = 1024 * 1024;
pub(crate) const MCP_FRAME_MAX: usize = 4 * 1024 * 1024;
pub(crate) const MCP_STDERR_TAIL: usize = 2 * 1024;
pub(crate) const MCP_DEFER_BYTES: usize = 8 * 1024;
pub(crate) const MCP_FOUND_MAX: usize = 8;
pub(crate) const LSP_START: Duration = Duration::from_secs(30);
pub(crate) const LSP_REQUEST: Duration = Duration::from_secs(30);
pub(crate) const LSP_CHECK: Duration = Duration::from_secs(5);
pub const LOG_RETAIN: Duration = Duration::from_secs(14 * 24 * 60 * 60);
pub(crate) const SPEED_WINDOW: Duration = Duration::from_secs(3);
pub(crate) const READER_WINDOW: Duration = Duration::from_millis(400);
pub(crate) const FRAME_WINDOW: Duration = Duration::from_secs(1);
pub(crate) const ANIMATION: Duration = Duration::from_millis(40);
pub(crate) const SPINNER: Duration = Duration::from_millis(100);
pub(crate) const SPLASH_REVEAL_MS: u128 = 760;
pub(crate) const STATUS_REFRESH: Duration = Duration::from_secs(1);
pub(crate) const BEAT: Duration = Duration::from_secs(2);
#[cfg(feature = "gui")]
pub(crate) const WORKER_STOP: Duration = Duration::from_secs(2);
pub(crate) const REPEAT_WINDOW: Duration = Duration::from_secs(2);
pub(crate) const RETRY_LIMIT: usize = 3;
pub(crate) const REPAIR_LIMIT: usize = 6;
pub(crate) const EDIT_HUNK_ROWS: usize = 80;
pub(crate) const SUMMARY_RETRY_ROUNDS: usize = 8;
pub(crate) const RETRY_BASE: Duration = Duration::from_secs(1);
pub(crate) const RETRY_MAX: Duration = Duration::from_secs(8);
pub(crate) const RETRY_AFTER_MAX: Duration = Duration::from_secs(60);

pub(crate) const fn seconds(value: u64) -> Duration {
    Duration::from_secs(value)
}

pub(crate) fn retry_policy() -> leyline::RetryPolicy {
    leyline::RetryPolicy::transient()
        .max_retries(RETRY_LIMIT as u32)
        .initial_backoff(RETRY_BASE)
        .max_backoff(RETRY_MAX)
        .max_retry_after(RETRY_AFTER_MAX)
        .retry_on([
            leyline::RetryTrigger::ConnectionError,
            leyline::RetryTrigger::Status(429),
            leyline::RetryTrigger::ServerError,
        ])
}
