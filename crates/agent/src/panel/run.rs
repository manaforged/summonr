use std::path::Path;

use super::{Content, Panel, program};
use crate::process::{Cap, Env, Spec};

/// The A2UI `action` message for a pressed button, as a panel command reads it on stdin.
#[must_use]
pub fn action(panel: &str, event: &str, context: serde_json::Value) -> String {
    serde_json::json!({"version": "1.0", "action": {
        "name": event,
        "surfaceId": panel,
        "timestamp": jiff::Timestamp::now().to_string(),
        "context": context,
    }})
    .to_string()
}

/// Starts a panel's command in the workspace with a clean environment.
pub(super) fn start(panel: &Panel, workspace: &Path) -> Result<crate::process::Process, String> {
    if let Some(problem) = &panel.problem {
        return Err(problem.clone());
    }
    let (word, args) = panel
        .command
        .split_first()
        .ok_or_else(|| "`command` is empty".to_string())?;
    let spec = Spec {
        cwd: Some(workspace.to_path_buf()),
        base: Env::Clean,
        ..Spec::new(
            program(word, panel.root.as_deref().unwrap_or(workspace)),
            args,
        )
    };
    crate::process::start(spec).map_err(|error| format!("did not start: {error}"))
}

/// Runs a data panel's command in the workspace and reads what it printed.
pub async fn run(
    panel: &Panel,
    workspace: &Path,
    input: Option<String>,
) -> Result<Content, String> {
    let process = start(panel, workspace)?;
    Content::parse(&output(process, input).await?)
}

/// Waits for a started command within the panel limits and returns what it printed.
pub(crate) async fn output(
    process: crate::process::Process,
    input: Option<String>,
) -> Result<String, String> {
    let limits = crate::limits::limits();
    let max = limits.panel_output_bytes;
    let keep = (Cap::Head(max + 1), Cap::stderr());
    let input = input.unwrap_or_default();
    let ran = crate::process::finish(
        process,
        input.as_bytes(),
        keep,
        limits.panel_timeout(),
        "the command",
    )
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;
    if ran.code != Some(0) {
        return Err(ran.reason());
    }
    if ran.stdout.len() > max {
        return Err(format!("printed more than {max} bytes"));
    }
    Ok(ran.stdout)
}
