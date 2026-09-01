use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::*;

fn values(rows: &[(&str, &str)]) -> BTreeMap<String, OsString> {
    rows.iter()
        .map(|(key, value)| (key.to_string(), OsString::from(value)))
        .collect()
}

#[test]
fn target_uses_the_herdr_binary_when_old_servers_do_not_export_one() {
    let env = values(&[
        ("HERDR_ENV", "1"),
        ("HERDR_PANE_ID", "w1:p2"),
        ("HERDR_SOCKET_PATH", "/tmp/herdr.sock"),
    ]);

    let target = target("session-1", Some(OsStr::new("summon")), |key| {
        env.get(key).cloned()
    })
    .expect("a Summoner process in a Herdr pane should report");

    assert_eq!(target.bin, OsStr::new("herdr"));
    assert_eq!(target.pane, OsStr::new("w1:p2"));
    assert_eq!(target.socket, OsStr::new("/tmp/herdr.sock"));
    assert_eq!(target.session, "session-1");
}

#[test]
fn target_prefers_the_binary_path_exported_by_newer_servers() {
    let env = values(&[
        ("HERDR_ENV", "1"),
        ("HERDR_PANE_ID", "w1:p2"),
        ("HERDR_SOCKET_PATH", "/tmp/herdr.sock"),
        ("HERDR_BIN_PATH", "/opt/homebrew/bin/herdr"),
    ]);

    let target = target("session-1", Some(OsStr::new("summoner")), |key| {
        env.get(key).cloned()
    })
    .expect("a Summoner process in a Herdr pane should report");

    assert_eq!(target.bin, OsStr::new("/opt/homebrew/bin/herdr"));
}

#[test]
fn target_ignores_other_processes_and_incomplete_herdr_environments() {
    let complete = values(&[
        ("HERDR_ENV", "1"),
        ("HERDR_PANE_ID", "w1:p2"),
        ("HERDR_SOCKET_PATH", "/tmp/herdr.sock"),
    ]);
    assert!(
        target("session-1", Some(OsStr::new("summoner-tests")), |key| {
            complete.get(key).cloned()
        })
        .is_none()
    );

    let incomplete = values(&[("HERDR_ENV", "1"), ("HERDR_PANE_ID", "w1:p2")]);
    assert!(
        target("session-1", Some(OsStr::new("summon")), |key| {
            incomplete.get(key).cloned()
        })
        .is_none()
    );
}

#[test]
fn product_ignores_test_artifacts_named_like_the_real_binary() {
    assert_eq!(
        product(Path::new("/home/user/.cargo/bin/summoner")),
        Some(OsStr::new("summoner"))
    );
    assert_eq!(
        product(Path::new("/cache/units/test-123/out/summoner")),
        None
    );
    assert_eq!(
        product(Path::new("/workspace/target/debug/deps/summon")),
        None
    );
}

#[test]
fn lifecycle_arguments_carry_state_and_session_identity() {
    let report = report_args(OsStr::new("w1:p2"), "working", "session-1");
    assert_eq!(
        report,
        vec![
            "pane",
            "report-agent",
            "w1:p2",
            "--source",
            "summoner:lifecycle",
            "--agent",
            "summoner",
            "--state",
            "working",
            "--agent-session-id",
            "session-1",
        ]
    );

    let release = release_args(OsStr::new("w1:p2"));
    assert_eq!(
        release,
        vec![
            "pane",
            "release-agent",
            "w1:p2",
            "--source",
            "summoner:lifecycle",
            "--agent",
            "summoner",
        ]
    );
}
