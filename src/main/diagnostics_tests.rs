use std::fs;
use std::process::{Command, Stdio};

use super::Log;

const PROBE: &str = "SUMMONER_PANIC_PROBE";
const SESSION_PROBE: &str = "SUMMONER_SESSION_PROBE";

fn log(home: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    fs::read_dir(home.join("logs"))?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "run log"))
}

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
    let text = fs::read_to_string(log(home.path())?)?;

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
    let text = fs::read_to_string(log(home.path())?)?;

    assert!(status.success());
    assert!(text.contains("\"event\":\"session\""), "{text}");
    assert!(text.contains("\"session\":\"session-sentinel\""), "{text}");
    assert!(text.contains("session-sentinel.jsonl"), "{text}");
    assert!(text.contains("workspace-sentinel"), "{text}");
    Ok(())
}

#[test]
fn dropped_log_is_incomplete() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let dir = home.path().join("logs");
    fs::create_dir_all(&dir)?;
    let path = dir.join("run.jsonl");
    let lock = super::lock(&path)?;
    super::append(&path, &serde_json::json!({ "event": "start" }))?;
    drop(lock);
    let runs = super::runs(&dir, None)?;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].path, path);
    assert_eq!(runs[0].status, super::RunStatus::Incomplete);
    Ok(())
}

#[test]
fn corrupt_log_is_incomplete() {
    let path = std::path::Path::new("run.jsonl");
    assert_eq!(super::status(path, b"\xff"), super::RunStatus::Incomplete);
    assert_eq!(
        super::status(path, b"{not json}\n{\"event\":\"exit\",\"code\":0}\n"),
        super::RunStatus::Incomplete
    );
}

#[test]
fn panic_outweighs_a_later_clean_exit() {
    let path = std::path::Path::new("run.jsonl");
    let bytes = b"{\"event\":\"panic\"}\n{\"event\":\"exit\",\"code\":0}\n";
    assert_eq!(super::status(path, bytes), super::RunStatus::Panic);
}

#[test]
fn sweep_removes_only_expired_inactive_runs() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let old = home.path().join("old.jsonl");
    let recent = home.path().join("recent.jsonl");
    fs::write(&old, "{\"event\":\"exit\",\"code\":0}\n")?;
    fs::write(&recent, "{\"event\":\"exit\",\"code\":0}\n")?;
    let expired = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(15 * 24 * 60 * 60))
        .expect("valid time");
    fs::File::options()
        .write(true)
        .open(&old)?
        .set_modified(expired)?;

    super::sweep(home.path());

    assert!(!old.exists());
    assert!(recent.exists());
    Ok(())
}
