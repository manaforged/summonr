use std::time::Duration;

const DAY_SECS: u64 = 24 * 60 * 60;

pub(super) fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

pub(super) fn millis(value: u64) -> Duration {
    Duration::from_millis(value)
}

pub(super) fn days(value: u64) -> Duration {
    Duration::from_secs(value.saturating_mul(DAY_SECS))
}
