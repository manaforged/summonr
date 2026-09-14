#![cfg(windows)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use tokio::process::Command;
use tokio::time::timeout;

fn powershell() -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot"))
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

fn literal(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

async fn run(workspace: &Path, readonly: bool, script: &str) -> Output {
    let script = format!(
        "$ErrorActionPreference = 'Stop'\n[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()\n{script}"
    );
    timeout(
        Duration::from_secs(30),
        Command::new(env!("CARGO_BIN_EXE_summonr"))
            .arg("--sandbox-exec")
            .arg(workspace)
            .arg(workspace)
            .arg(readonly.to_string())
            .arg("true")
            .arg(std::process::id().to_string())
            .arg(powershell())
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .current_dir(workspace)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("sandbox exceeded 30 seconds")
    .expect("run sandbox")
}

#[tokio::test]
async fn workspace_writes_preserve_quoted_paths_and_unicode_output() {
    let root = tempfile::Builder::new()
        .prefix("summonr's sandbox ")
        .tempdir()
        .expect("tempdir");
    let file = root.path().join("result.txt");
    let output = run(
        root.path(),
        false,
        &format!(
            "[System.IO.File]::WriteAllText('{}', 'café')\n[Console]::Write('café')",
            literal(&file)
        ),
    )
    .await;
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(String::from_utf8(output.stdout).expect("UTF-8"), "café");
    assert_eq!(fs::read_to_string(file).expect("written file"), "café");
}

#[tokio::test]
async fn readonly_commands_cannot_overwrite_workspace_files() {
    let root = tempfile::tempdir().expect("tempdir");
    let file = root.path().join("protected.txt");
    fs::write(&file, "original").expect("fixture");
    let output = run(
        root.path(),
        true,
        &format!(
            "[Console]::Write('command started')\n[System.IO.File]::WriteAllText('{}', 'changed')",
            literal(&file)
        ),
    )
    .await;
    assert_eq!(output.stdout, b"command started", "{output:?}");
    assert_ne!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fs::read_to_string(file).expect("protected file"),
        "original"
    );
}

#[tokio::test]
async fn writable_commands_cannot_overwrite_files_outside_the_workspace() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).expect("workspace");
    let file = root.path().join("outside.txt");
    fs::write(&file, "original").expect("fixture");
    let output = run(
        &workspace,
        false,
        &format!(
            "[Console]::Write('command started')\n[System.IO.File]::WriteAllText('{}', 'changed')",
            literal(&file)
        ),
    )
    .await;
    assert_eq!(output.stdout, b"command started", "{output:?}");
    assert_ne!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(fs::read_to_string(file).expect("outside file"), "original");
}

#[tokio::test]
async fn descendants_exit_when_the_command_finishes() {
    let root = tempfile::tempdir().expect("tempdir");
    let ready = root.path().join("child.pid");
    let child = format!(
        "[System.IO.File]::WriteAllText('{}', [string]$PID)\nStart-Sleep -Seconds 30",
        literal(&ready)
    );
    let bytes: Vec<u8> = child.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = STANDARD.encode(bytes);
    let output = run(
        root.path(),
        false,
        &format!(
            "$child = Start-Process -FilePath '{}' -ArgumentList '-NoProfile -NonInteractive -EncodedCommand {encoded}' -PassThru\n$deadline = [DateTime]::UtcNow.AddSeconds(10)\nwhile (!(Test-Path -LiteralPath '{}')) {{\nif ($child.HasExited -or [DateTime]::UtcNow -gt $deadline) {{ throw 'child did not start' }}\nStart-Sleep -Milliseconds 20\n}}\n[Console]::Write($child.Id)",
            literal(&powershell()),
            literal(&ready)
        ),
    )
    .await;
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let pid: u32 = String::from_utf8(output.stdout)
        .expect("child PID")
        .parse()
        .expect("numeric child PID");
    assert_eq!(
        fs::read_to_string(ready).expect("child started"),
        pid.to_string()
    );
    let status = timeout(
        Duration::from_secs(10),
        Command::new(powershell())
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "Wait-Process -Id {pid} -Timeout 3 -ErrorAction SilentlyContinue\nif (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ exit 1 }}\nexit 0"
                ),
            ])
            .kill_on_drop(true)
            .status(),
    )
    .await
    .expect("process check exceeded 10 seconds")
    .expect("check descendant");
    assert_eq!(status.code(), Some(0), "sandbox left process {pid} running");
}
