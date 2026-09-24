use std::time::Duration;

#[cfg(not(test))]
pub(crate) const JOB_WAIT_SECS: u64 = 120;
#[cfg(test)]
pub(crate) const JOB_WAIT_SECS: u64 = 1;
pub(crate) const JOB_WAIT: Duration = Duration::from_secs(JOB_WAIT_SECS);
pub(crate) const CHILD_POLL: Duration = Duration::from_secs(1);
pub(crate) const GRANT_WAIT: Duration = Duration::from_secs(600);
pub(crate) const GRANT_POLL: Duration = Duration::from_millis(60);
pub(crate) const STREAM_TOTAL: Duration = Duration::from_secs(24 * 60 * 60);
pub(crate) const STREAM_IDLE: Duration = JOB_WAIT.saturating_mul(2);
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
pub(crate) const LSP_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const ROOM_TICK: Duration = Duration::from_millis(500);
pub(crate) const ROOM_REPEAT: Duration = Duration::from_secs(5);
pub(crate) const ROOM_BURST_WINDOW: Duration = Duration::from_secs(10);
pub const LOG_RETAIN: Duration = Duration::from_secs(14 * 24 * 60 * 60);
pub(crate) const SPEED_WINDOW: Duration = Duration::from_secs(3);
pub(crate) const READER_WINDOW: Duration = Duration::from_millis(400);
pub(crate) const FRAME_WINDOW: Duration = Duration::from_secs(1);
pub(crate) const ANIMATION: Duration = Duration::from_millis(40);
pub(crate) const IDLE_POLL: Duration = Duration::from_millis(250);
pub(crate) const STATUS_REFRESH: Duration = Duration::from_secs(1);
pub(crate) const BEAT: Duration = Duration::from_secs(2);
#[cfg(feature = "gui")]
pub(crate) const WORKER_STOP: Duration = Duration::from_secs(2);
pub(crate) const REPEAT_WINDOW: Duration = Duration::from_secs(2);
pub(crate) const RETRY_LIMIT: usize = 3;
pub(crate) const REPAIR_LIMIT: usize = 6;
pub(crate) const RETRY_BASE: Duration = Duration::from_secs(1);
pub(crate) const RETRY_MAX: Duration = Duration::from_secs(8);
pub(crate) const RETRY_AFTER_MAX: Duration = Duration::from_secs(60);

pub(crate) const fn seconds(value: u64) -> Duration {
    Duration::from_secs(value)
}

pub(crate) fn retry_policy() -> leyline::RetryPolicy {
    leyline::RetryPolicy::transient()
        .with_max_retries(RETRY_LIMIT as u32)
        .with_backoff(RETRY_BASE, RETRY_MAX)
        .with_max_retry_after(RETRY_AFTER_MAX)
        .retry_on([
            leyline::RetryTrigger::ConnectionError,
            leyline::RetryTrigger::Status(429),
            leyline::RetryTrigger::ServerError,
        ])
}
