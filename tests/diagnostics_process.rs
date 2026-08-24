use std::fs;
use std::process::Stdio;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use tokio::process::Command;
use tokio::time::sleep;

#[tokio::test]
async fn completed_command_records_exit() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let status = Command::new(env!("CARGO_BIN_EXE_summoner"))
        .arg("sessions")
        .env("MAGE_HOME", home.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;
    let text = fs::read_to_string(home.path().join("logs/summoner.jsonl"))?;

    assert!(status.success());
    assert!(text.contains("\"event\":\"start\""), "{text}");
    assert!(text.contains("\"event\":\"exit\""), "{text}");
    assert!(text.contains("\"code\":0"), "{text}");
    Ok(())
}

#[tokio::test]
async fn failed_command_records_error() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let status = Command::new(env!("CARGO_BIN_EXE_summoner"))
        .args(["qwen", "--session", "missing", "-p", "hello"])
        .env("MAGE_HOME", home.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;
    let text = fs::read_to_string(home.path().join("logs/summoner.jsonl"))?;

    assert!(!status.success());
    assert!(text.contains("\"event\":\"exit\""), "{text}");
    assert!(text.contains("\"code\":1"), "{text}");
    assert!(text.contains("no session `missing`"), "{text}");
    Ok(())
}

#[tokio::test]
async fn killed_harness_leaves_an_open_run() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let log = home.path().join("logs/summoner.jsonl");
    let mut child = Command::new(env!("CARGO_BIN_EXE_summoner"))
        .args(["qwen", "--no-tui"])
        .env("MAGE_HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| std::io::Error::other("Summoner started without a process id"))?;

    let mut ready = false;
    for _ in 0..200 {
        if fs::metadata(&log).is_ok_and(|meta| meta.len() > 0) {
            ready = true;
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }

    child.kill().await?;
    let status = child.wait().await?;
    let text = fs::read_to_string(&log).unwrap_or_default();

    assert!(ready, "Summoner did not create {}", log.display());
    assert!(
        text.contains(&format!("\"pid\":{pid}")),
        "run log did not identify the killed process: {text}"
    );
    assert!(
        !text.contains("\"event\":\"exit\""),
        "killed run was recorded as a normal exit: {text}"
    );
    #[cfg(unix)]
    assert_eq!(status.signal(), Some(9));
    Ok(())
}
