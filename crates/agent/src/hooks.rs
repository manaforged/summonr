use crate::process::{Cap, Env, Process, Spec, Tail};
use serde_json::Value;
use std::path::Path;
use tokio::io::AsyncWriteExt;

mod judge;

pub(crate) use judge::Verdict;
use judge::{Outcome, failed, judge};
pub(crate) use summonr_types::hooks::{
    Call, Compaction, Dialect, End, Failure, Hook, HookEvent, Hooks, Matcher, Notice, Permission,
    Start, builtin,
};

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
    let spec = Spec {
        cwd: Some(workspace.to_path_buf()),
        base: Env::Clean,
        ..Spec::new(
            program,
            summonr_core::shell::arguments(&hook.command, workspace),
        )
    }
    .envs(crate::plugins::hook_env(workspace, hook.root.as_deref()))
    .envs(summonr_core::dirs::summonr_home().map(|home| (summonr_core::dirs::HOME_ENV, home)));
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
    let limit = hook
        .timeout
        .unwrap_or_else(|| crate::limits::limits().job_wait());
    let finished = crate::limits::within(limit, format!("hook `{}`", hook.command), work).await;
    drop(group);
    finished
        .map_err(|error| error.to_string())?
        .map_err(|error| format!("hook `{}` failed: {error}", hook.command))
}

pub(crate) async fn run(hook: &Hook, call: &Call<'_>) -> Verdict {
    let body = match hook.dialect {
        Dialect::Summonr => call.payload.clone(),
        Dialect::Claude => hook.claude_payload(call),
    };
    match execute(hook, call.workspace, &body).await {
        Ok(outcome) => judge(hook.dialect, call.event, &outcome),
        Err(error) => failed(hook.dialect, call.event, error),
    }
}
