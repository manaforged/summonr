use crate::process::{Cap, Env, Process, Spec, Tail};
use serde_json::Value;
use std::path::Path;
use tokio::io::AsyncWriteExt;

pub(crate) use summonr_types::hooks::{Call, Dialect, Hook, HookEvent, Hooks, Matcher, builtin};

#[must_use]
pub(crate) fn with_plugins(mut hooks: Hooks, plugins: &[crate::plugins::Plugin]) -> Hooks {
    for plugin in plugins {
        let extra = plugin.hooks();
        for event in HookEvent::ALL {
            for hook in extra.of(event) {
                hooks.push(event, hook.clone());
            }
        }
    }
    hooks
}

async fn execute(hook: &Hook, workspace: &Path, body: &Value) -> Result<Outcome, String> {
    let program = summonr_core::shell::program().map_err(|error| error.to_string())?;
    let root = hook.root.as_ref();
    let data = root
        .and_then(|root| root.file_name())
        .and_then(|name| crate::plugins::data_dir(&name.to_string_lossy()).ok());
    let spec = Spec {
        cwd: Some(workspace.to_path_buf()),
        base: Env::Clean,
        ..Spec::new(
            program,
            summonr_core::shell::arguments(&hook.command, workspace),
        )
    }
    .envs([("CLAUDE_PROJECT_DIR", workspace.as_os_str())])
    .envs(summonr_core::dirs::summonr_home().map(|home| ("SUMMONR_HOME", home)))
    .envs(
        root.into_iter()
            .flat_map(|root| [("CLAUDE_PLUGIN_ROOT", root), ("SUMMONR_PLUGIN_ROOT", root)]),
    )
    .envs(
        data.iter()
            .flat_map(|data| [("CLAUDE_PLUGIN_DATA", data), ("SUMMONR_PLUGIN_DATA", data)]),
    );
    let Process {
        mut child,
        mut stdin,
        stdout,
        stderr,
        group,
    } = crate::process::start(spec)
        .map_err(|error| format!("hook `{}` did not start: {error}", hook.command))?;
    let text = body.to_string();
    let max = crate::limits::limits().hook_output_max_bytes;
    let mut stdout = Tail::spawn(Some(stdout), Cap::Head(max));
    let mut stderr = Tail::spawn(stderr, Cap::Head(max));
    let work = async move {
        drop(stdin.write_all(text.as_bytes()).await);
        drop(stdin);
        let status = child.wait().await?;
        drop(stdout.settle().await);
        drop(stderr.settle().await);
        Ok::<_, std::io::Error>(Outcome {
            code: status.code(),
            stdout: stdout.text().trim().to_string(),
            stderr: stderr.text().trim().to_string(),
        })
    };
    let finished = tokio::time::timeout(crate::limits::limits().job_wait(), work).await;
    drop(group);
    finished
        .map_err(|_elapsed| {
            format!(
                "hook `{}` ran past {}s",
                hook.command,
                crate::limits::limits().job_wait_secs
            )
        })?
        .map_err(|error| format!("hook `{}` failed: {error}", hook.command))
}

pub(crate) enum Verdict {
    Allow,
    Block(String),
    Note(String),
}

pub(crate) async fn run(hook: &Hook, call: &Call<'_>) -> Verdict {
    let body = match hook.dialect {
        Dialect::Summonr => call.payload.clone(),
        Dialect::Claude => hook.claude_payload(call),
    };
    let outcome = match execute(hook, call.workspace, &body).await {
        Ok(outcome) => outcome,
        Err(error) if call.event == HookEvent::PreTool => return Verdict::Block(error),
        Err(error) => return Verdict::Note(error),
    };
    judge(hook.dialect, call.event, outcome)
}

struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn judge(dialect: Dialect, event: HookEvent, outcome: Outcome) -> Verdict {
    let reason = if outcome.stderr.is_empty() {
        format!("exited with {:?}", outcome.code)
    } else {
        outcome.stderr.clone()
    };
    match (dialect, outcome.code) {
        (Dialect::Summonr, Some(0)) => Verdict::Allow,
        (Dialect::Summonr, Some(_) | None) => Verdict::Block(reason),
        (Dialect::Claude, Some(2)) if event == HookEvent::PreTool => Verdict::Block(reason),
        (Dialect::Claude, Some(0)) => {
            denied(&outcome.stdout).map_or(Verdict::Allow, Verdict::Block)
        }
        (Dialect::Claude, Some(_) | None) => Verdict::Note(reason),
    }
}

fn denied(stdout: &str) -> Option<String> {
    let value: Value = serde_json::from_str(stdout).ok()?;
    let output = value.get("hookSpecificOutput")?;
    (output.get("permissionDecision")?.as_str()? == "deny").then(|| {
        output
            .get("permissionDecisionReason")
            .and_then(Value::as_str)
            .unwrap_or("denied by a plugin hook")
            .to_string()
    })
}
