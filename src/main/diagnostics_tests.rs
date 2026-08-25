use std::fs;
use std::process::{Command, Stdio};

use super::Log;

const PROBE: &str = "SUMMONER_PANIC_PROBE";
const SESSION_PROBE: &str = "SUMMONER_SESSION_PROBE";

#[test]
fn panic_is_written() -> std::io::Result<()> {
    if std::env::var_os(PROBE).is_some() {
        let _log = Log::start();
        panic!("diagnostic panic sentinel");
    }

    let home = tempfile::tempdir()?;
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", "diagnostics::tests::panic_is_written"])
        .env("MAGE_HOME", home.path())
        .env(PROBE, "1")
        .output()?;
    let text = fs::read_to_string(home.path().join("logs/summoner.jsonl"))?;

    assert!(!output.status.success());
    assert!(text.contains("\"event\":\"panic\""), "{text}");
    assert!(text.contains("diagnostic panic sentinel"), "{text}");
    assert!(text.contains("\"backtrace\":"), "{text}");
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.contains("crash recorded in"), "{output}");
    Ok(())
}

#[test]
fn session_is_correlated_with_the_run() -> std::io::Result<()> {
    if std::env::var_os(SESSION_PROBE).is_some() {
        let log = Log::start().expect("probe must create a diagnostic log");
        let temp = std::env::temp_dir();
        log.session(
            "session-sentinel",
            &temp.join("session-sentinel.jsonl"),
            &temp.join("workspace-sentinel"),
        );
        log.finish(0, None);
        return Ok(());
    }

    let home = tempfile::tempdir()?;
    let status = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "diagnostics::tests::session_is_correlated_with_the_run",
        ])
        .env("MAGE_HOME", home.path())
        .env(SESSION_PROBE, "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    let text = fs::read_to_string(home.path().join("logs/summoner.jsonl"))?;

    assert!(status.success());
    assert!(text.contains("\"event\":\"session\""), "{text}");
    assert!(text.contains("\"session\":\"session-sentinel\""), "{text}");
    assert!(text.contains("session-sentinel.jsonl"), "{text}");
    assert!(text.contains("workspace-sentinel"), "{text}");
    Ok(())
}
