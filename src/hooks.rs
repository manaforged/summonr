use std::path::Path;
use std::process::Stdio;

use serde::Deserialize;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::runtime::JOB_WAIT;

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Hooks {
    pub pre_tool: Option<String>,
    pub post_tool: Option<String>,
    pub compact: Option<String>,
    pub stop: Option<String>,
}

pub(crate) async fn run(command: &str, workspace: &Path, payload: &Value) -> Result<(), String> {
    let program = crate::shell::program().map_err(|error| error.to_string())?;
    let mut child = Command::new(program)
        .args(crate::shell::arguments(command, workspace))
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("hook `{command}` did not start: {error}"))?;
    let body = payload.to_string();
    let work = async move {
        if let Some(mut stdin) = child.stdin.take() {
            drop(stdin.write_all(body.as_bytes()).await);
        }
        child.wait_with_output().await
    };
    let output = tokio::time::timeout(JOB_WAIT, work)
        .await
        .map_err(|_elapsed| format!("hook `{command}` ran past {}s", JOB_WAIT.as_secs()))?
        .map_err(|error| format!("hook `{command}` failed: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        format!("hook `{command}` exited with {}", output.status)
    } else {
        stderr
    })
}
