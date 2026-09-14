use std::fs;
use std::process::Stdio;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use tokio::process::Command;
use tokio::time::{sleep, timeout};

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
    let status = Command::new(env!("CARGO_BIN_EXE_summonr"))
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
    let status = Command::new(env!("CARGO_BIN_EXE_summonr"))
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
async fn failed_workflow_records_error_without_activation() -> Result<(), Box<dyn std::error::Error>>
{
    let home = tempfile::tempdir()?;
    let workspace = tempfile::tempdir()?;
    fs::write(
        home.path().join("config.toml"),
        "[endpoint.fixture]\nbase_url = 'http://127.0.0.1:1/v1'\nmodel = 'fixture'\n\
         [workflows.broken]\nreview = 'missing-reviewer-endpoint'\n",
    )?;
    let output = timeout(
        Duration::from_secs(15),
        Command::new(env!("CARGO_BIN_EXE_summonr"))
            .args([
                "fixture",
                "--here",
                "--no-sandbox",
                "-w",
                "broken",
                "-p",
                "hello",
            ])
            .current_dir(workspace.path())
            .env("MAGE_HOME", home.path())
            .env("MAGE_API_KEY", "fixture")
            .env("MAGE_BUS", "off")
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unknown endpoint `missing-reviewer-endpoint`")
    );
    let text = fs::read_to_string(log(home.path())?)?;
    assert!(text.contains("\"code\":1"), "{text}");
    for entry in fs::read_dir(home.path().join("sessions"))? {
        let path = entry?.path().join("session.jsonl");
        if path.is_file() {
            let session = fs::read_to_string(path)?;
            assert!(!session.contains("\"kind\":\"workflow\""), "{session}");
        }
    }
    Ok(())
}

#[tokio::test]
async fn killed_harness_leaves_an_open_run() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let mut child = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(["qwen"])
        .env("MAGE_HOME", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let pid = child
        .id()
        .ok_or_else(|| std::io::Error::other("Summonr started without a process id"))?;

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

    assert!(ready, "Summonr did not create {}", log.display());
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
    assert!(!status.success(), "killed Summonr exited successfully");
    Ok(())
}

#[tokio::test]
async fn logs_lists_a_completed_run() -> std::io::Result<()> {
    let home = tempfile::tempdir()?;
    let status = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .arg("sessions")
        .env("MAGE_HOME", home.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;
    assert!(status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
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

    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
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
    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
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
    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .arg("--version")
        .output()
        .await?;
    let stdout = String::from_utf8(output.stdout).map_err(std::io::Error::other)?;

    assert!(output.status.success());
    assert!(stdout.starts_with("summonr 0.1.0 ("), "{stdout}");
    assert!(!stdout.contains("unknown"), "{stdout}");
    Ok(())
}
