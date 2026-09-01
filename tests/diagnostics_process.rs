use std::fs;
use std::process::Stdio;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use tokio::process::Command;
use tokio::time::sleep;

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
    let text = fs::read_to_string(log(home.path())?)?;

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
    let text = fs::read_to_string(log(home.path())?)?;

    assert!(!status.success());
    assert!(text.contains("\"event\":\"exit\""), "{text}");
    assert!(text.contains("\"code\":1"), "{text}");
    assert!(text.contains("no session `missing`"), "{text}");
    Ok(())
}

#[tokio::test]
async fn killed_harness_leaves_an_open_run() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_summoner"))
        .args(["qwen"])
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
        if log(home.path()).is_ok_and(|path| fs::metadata(path).is_ok_and(|meta| meta.len() > 0)) {
            ready = true;
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }

    child.kill().await?;
    let status = child.wait().await?;
    let log = log(home.path())?;
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
    #[cfg(not(unix))]
    assert!(!status.success(), "killed Summoner exited successfully");
    Ok(())
}

#[tokio::test]
async fn logs_lists_a_completed_run() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let status = Command::new(env!("CARGO_BIN_EXE_summoner"))
        .arg("sessions")
        .env("MAGE_HOME", home.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;
    assert!(status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_summoner"))
        .arg("logs")
        .env("MAGE_HOME", home.path())
        .output()
        .await?;
    let stdout = String::from_utf8(output.stdout).map_err(std::io::Error::other)?;
    assert!(output.status.success());
    assert!(stdout.contains("complete"), "{stdout}");
    assert!(stdout.contains(".jsonl"), "{stdout}");
    Ok(())
}

#[tokio::test]
async fn doctor_reports_a_killed_run() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let dir = home.path().join("logs");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("killed.jsonl"), "{\"event\":\"start\"}\n")?;

    let output = Command::new(env!("CARGO_BIN_EXE_summoner"))
        .arg("doctor")
        .env("MAGE_HOME", home.path())
        .output()
        .await?;
    let stdout = String::from_utf8(output.stdout).map_err(std::io::Error::other)?;
    assert!(!output.status.success());
    assert!(stdout.contains("incomplete runs\t1"), "{stdout}");
    assert!(stdout.contains("binary\t"), "{stdout}");
    Ok(())
}

#[tokio::test]
async fn help_does_not_create_a_run_log() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let output = Command::new(env!("CARGO_BIN_EXE_summoner"))
        .arg("--help")
        .env("MAGE_HOME", home.path())
        .output()
        .await?;

    assert!(output.status.success());
    assert!(!home.path().join("logs").exists());
    Ok(())
}

#[tokio::test]
async fn version_identifies_the_source_revision() -> std::io::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_summoner"))
        .arg("--version")
        .output()
        .await?;
    let stdout = String::from_utf8(output.stdout).map_err(std::io::Error::other)?;

    assert!(output.status.success());
    assert!(stdout.starts_with("summoner 0.1.0 ("), "{stdout}");
    assert!(!stdout.contains("unknown"), "{stdout}");
    Ok(())
}
