use std::fs;
use std::path::Path;
use std::process::Output;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use super::{arguments, program, wrap};

async fn run(script: &str, cwd: &Path) -> Output {
    timeout(
        Duration::from_secs(15),
        Command::new(program().expect("shell"))
            .args(arguments(script, cwd))
            .current_dir(cwd)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .expect("shell exceeded 15 seconds")
    .expect("run shell")
}

#[tokio::test]
async fn foreground_reads_literal_text_in_the_working_directory() {
    let dir = tempfile::Builder::new()
        .prefix("summonr's shell ")
        .tempdir()
        .expect("tempdir");
    let text = "literal ' \" café [] $&";
    fs::write(dir.path().join("input.txt"), text).expect("input");
    #[cfg(unix)]
    let script = "cat input.txt";
    #[cfg(windows)]
    let script = "[Console]::Write((Get-Content -Raw -Encoding UTF8 -LiteralPath 'input.txt'))";
    let output = run(
        script,
        &dir.path().canonicalize().expect("canonical workspace"),
    )
    .await;
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 output"),
        text
    );
}

#[tokio::test]
async fn background_records_failure_in_a_quoted_path() {
    let dir = tempfile::Builder::new()
        .prefix("summonr's jobs ")
        .tempdir()
        .expect("tempdir");
    let cwd = dir.path().canonicalize().expect("canonical workspace");
    let exit = cwd.join("job.exit");
    let output = run(&wrap("exit 7", &exit, &cwd), &cwd).await;
    assert_eq!(output.status.code(), Some(7), "{output:?}");
    assert_eq!(fs::read_to_string(exit).expect("exit marker"), "7\n");
}

#[tokio::test]
async fn background_reads_relative_paths_before_recording_success() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("input.txt"), "background output").expect("input");
    let cwd = dir.path().canonicalize().expect("canonical workspace");
    let exit = cwd.join("job.exit");
    #[cfg(unix)]
    let script = "cat input.txt";
    #[cfg(windows)]
    let script = "[Console]::Write((Get-Content -Raw -LiteralPath 'input.txt'))";
    let output = run(&wrap(script, &exit, &cwd), &cwd).await;
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(output.stdout, b"background output");
    assert_eq!(fs::read_to_string(exit).expect("exit marker"), "0\n");
}
