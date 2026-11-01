/// Seconds since the Unix epoch, or 0 when the clock is before it.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

/// Milliseconds since the Unix epoch, or the largest value on overflow.
pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(u64::MAX, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

#[path = "clock_tests.rs"]
mod tests;
