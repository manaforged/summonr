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

    let target = target("session-1", "grok-4", Some(OsStr::new("summon")), |key| {
        env.get(key).cloned()
    })
    .expect("a Summonr process in a Herdr pane should report");

    assert_eq!(target.bin, OsStr::new("herdr"));
    assert_eq!(target.pane, OsStr::new("w1:p2"));
    assert_eq!(target.socket, OsStr::new("/tmp/herdr.sock"));
    assert_eq!(held(&target.session), "session-1");
}

#[test]
fn target_prefers_the_binary_path_exported_by_newer_servers() {
    let env = values(&[
        ("HERDR_ENV", "1"),
        ("HERDR_PANE_ID", "w1:p2"),
        ("HERDR_SOCKET_PATH", "/tmp/herdr.sock"),
        ("HERDR_BIN_PATH", "/opt/homebrew/bin/herdr"),
    ]);

    let target = target("session-1", "grok-4", Some(OsStr::new("summoner")), |key| {
        env.get(key).cloned()
    })
    .expect("a Summonr process in a Herdr pane should report");

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
        target(
            "session-1",
            "grok-4",
            Some(OsStr::new("summonr-tests")),
            |key| complete.get(key).cloned(),
        )
        .is_none()
    );

    let incomplete = values(&[("HERDR_ENV", "1"), ("HERDR_PANE_ID", "w1:p2")]);
    assert!(
        target("session-1", "grok-4", Some(OsStr::new("summon")), |key| {
            incomplete.get(key).cloned()
        })
        .is_none()
    );
}

#[test]
fn product_ignores_test_artifacts_named_like_the_real_binary() {
    assert_eq!(
        product(Path::new("/usr/local/bin/summoner")),
        Some(OsStr::new("summoner"))
    );
    assert_eq!(
        product(Path::new("/usr/local/bin/summonr")),
        Some(OsStr::new("summonr"))
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
    let report = report_args(OsStr::new("w1:p2"), "working", "session-1", 3);
    assert_eq!(
        report,
        vec![
            "pane",
            "report-agent",
            "w1:p2",
            "--source",
            "summonr:lifecycle",
            "--agent",
            "summonr",
            "--state",
            "working",
            "--agent-session-id",
            "session-1",
            "--seq",
            "3",
        ]
    );

    let release = release_args(OsStr::new("w1:p2"), 4);
    assert_eq!(
        release,
        vec![
            "pane",
            "release-agent",
            "w1:p2",
            "--source",
            "summonr:lifecycle",
            "--agent",
            "summonr",
            "--seq",
            "4",
        ]
    );
}

#[test]
fn display_arguments_carry_title_and_tokens() {
    let args = metadata_args(OsStr::new("w1:p2"), "auth review", "grok-4", "session-1", 2);
    assert_eq!(
        args,
        vec![
            "pane",
            "report-metadata",
            "w1:p2",
            "--source",
            "summonr:display",
            "--agent",
            "summonr",
            "--title",
            "auth review",
            "--display-agent",
            "summonr",
            "--token",
            "model=grok-4",
            "--token",
            "session=session-1",
            "--token",
            "summary=auth review",
            "--seq",
            "2",
        ]
    );
}
