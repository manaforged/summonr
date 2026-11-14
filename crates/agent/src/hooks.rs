use crate::process::{Cap, Env, Process, Spec, Tail};
use serde_json::Value;
use std::path::Path;
use tokio::io::AsyncWriteExt;

mod judge;

pub(crate) use judge::Verdict;
use judge::{Outcome, failed, judge};
pub(crate) use summonr_types::hooks::{
    Call, Compaction, Dialect, End, Hook, HookEvent, Hooks, Matcher, Permission, Start, builtin,
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
    .envs(summonr_core::dirs::summonr_home().map(|home| (summonr_core::dirs::HOME_ENV, home)))
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
