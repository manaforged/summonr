use std::ffi::OsString;
use std::path::Path;

use super::*;

fn fake_herdr(dir: &Path) -> (OsString, std::path::PathBuf) {
    let log = dir.join("calls.log");
    let bin = dir.join("herdr");
    std::fs::write(
        &bin,
        format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    (bin.into_os_string(), log)
}

fn target_in(dir: &Path, session: &str) -> Target {
    let (bin, _) = fake_herdr(dir);
    super::target(
        session,
        Path::new("/work/app"),
        "deepseek/flash",
        Some(OsStr::new("summonr")),
        |key| match key {
            "HERDR_ENV" => Some("1".into()),
            "HERDR_PANE_ID" => Some("pane-7".into()),
            "HERDR_SOCKET_PATH" => Some("/tmp/herdr.sock".into()),
            "HERDR_BIN_PATH" => Some(bin.clone()),
            _ => None,
        },
    )
    .unwrap()
}

#[test]
fn a_wait_for_approval_reports_blocked_with_why_and_how_to_resume() {
    let dir = tempfile::tempdir().unwrap();
    let target = Arc::new(target_in(dir.path(), "s42"));
    let reporter = Reporter {
        target: Some(Arc::clone(&target)),
    };
    drop(reporter.blocked("run `rm -rf build`?"));
    drop(reporter);
    let calls = std::fs::read_to_string(dir.path().join("calls.log")).unwrap();
    let lines: Vec<&str> = calls.lines().collect();
    let [blocked, working, released] = lines.as_slice() else {
        panic!("{calls}");
    };
    assert!(
        blocked.starts_with("pane report-agent pane-7 --source summonr:lifecycle --agent summonr --state blocked --message run `rm -rf build`? --seq "),
        "{blocked}"
    );
    assert!(
        blocked.ends_with("--agent-session-id s42 -- summonr resume s42"),
        "{blocked}"
    );
    assert!(working.contains("--state working"), "{working}");
    assert!(
        released.starts_with("pane release-agent pane-7 --source summonr:lifecycle"),
        "{released}"
    );
}

#[test]
fn a_new_process_reports_a_higher_seq_than_the_last_one_in_the_pane() {
    let earlier = AtomicU64::new(0);
    let first = next_seq(&earlier);
    let restarted = AtomicU64::new(0);
    let second = next_seq(&restarted);
    let third = next_seq(&restarted);
    assert!(second >= first, "{first} then {second}");
    assert!(third > second);
    assert!(first > 1_600_000_000_000, "{first}");
}
