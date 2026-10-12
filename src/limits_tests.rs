use std::time::Duration;

use super::{Limits, Overrides, limits};
use crate::endpoint::CacheTtl;

#[test]
fn the_built_in_limits_come_from_the_data_file() {
    let built = Limits::built_in();
    assert_eq!(built.clone().overlay(&Overrides::default()), built);
    assert_eq!(built.cache_ttl, CacheTtl::OneHour);
    assert_eq!(built.retry_after_max(), Duration::from_secs(60));
    assert_eq!(built.stream_idle(), Duration::from_secs(240));
    assert_eq!(built.mcp_call(), Duration::from_secs(300));
    assert_eq!(built.keychain(), Duration::from_secs(5));
    assert_eq!(
        built.session_retain(),
        Duration::from_secs(30 * 24 * 60 * 60)
    );
}

#[test]
fn a_limits_table_in_config_overrides_only_the_keys_it_names() {
    let file: crate::config::File =
        toml::from_str("[limits]\nretry_after_max_secs = 30\ncache_ttl = \"5m\"\n").unwrap();
    let tuned = Limits::built_in().overlay(&file.limits.unwrap());
    assert_eq!(tuned.retry_after_max(), Duration::from_secs(30));
    assert_eq!(tuned.cache_ttl, CacheTtl::FiveMinutes);
    assert_eq!(tuned.connect(), Limits::built_in().connect());
}

#[test]
fn an_endpoint_without_a_ttl_takes_the_limits_default() {
    let anthropic = crate::endpoint::resolve(Some("anthropic"), None, crate::Need::Config).unwrap();
    assert_eq!(anthropic.cache_ttl, limits().cache_ttl);
    let plan = crate::endpoint::resolve(Some("minimax-plan"), None, crate::Need::Config).unwrap();
    assert_eq!(plan.cache_ttl, CacheTtl::FiveMinutes);
}

#[test]
fn tests_load_the_built_in_limits_with_the_test_overlay() {
    let built = Limits::built_in();
    let (job, wait, kill) = if cfg!(windows) {
        (10, 10, 30)
    } else {
        (1, 1, 4)
    };
    assert_eq!(limits().job_wait_secs, job);
    assert_eq!(limits().command_wait_secs, wait);
    assert_eq!(limits().command_kill_secs, kill);
    assert_eq!(limits().keychain(), Duration::from_millis(600));
    assert_eq!(limits().mcp_call(), built.mcp_call());
    assert_ne!(built.job_wait_secs, limits().job_wait_secs);
}

#[test]
fn a_limits_table_in_config_overrides_a_moved_timing() {
    let file: crate::config::File = toml::from_str("[limits]\nmcp_call_secs = 45\n").unwrap();
    let tuned = Limits::built_in().overlay(&file.limits.unwrap());
    assert_eq!(tuned.mcp_call(), Duration::from_secs(45));
    assert_eq!(tuned.mcp_idle(), Limits::built_in().mcp_idle());
}
