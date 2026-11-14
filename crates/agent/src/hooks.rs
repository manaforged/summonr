use serde_json::Value;

use crate::process::Cap;

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
        hooks.extend(&plugin.hooks());
    }
    hooks
}

async fn execute(hook: &Hook, call: &Call<'_>, body: &Value) -> Result<Outcome, String> {
    let workspace = call.workspace;
    let spec = crate::process::shell(&hook.command, workspace)
        .map_err(|error| error.to_string())?
        .envs(call.env.iter().cloned())
        .envs(crate::plugins::hook_env(workspace, hook.root.as_deref()))
        .envs(summonr_core::dirs::summonr_home().map(|home| (summonr_core::dirs::HOME_ENV, home)));
    let process = crate::process::start(spec)
        .map_err(|error| format!("hook `{}` did not start: {error}", hook.command))?;
    let limit = hook
        .timeout
        .unwrap_or_else(|| crate::limits::limits().job_wait());
    let max = crate::limits::limits().hook_output_max_bytes;
    let what = format!("hook `{}`", hook.command);
    let keep = (Cap::Head(max), Cap::Head(max));
    crate::process::finish(process, body.to_string().as_bytes(), keep, limit, what)
        .await
        .map_err(|error| error.to_string())?
        .map(crate::process::Ran::trimmed)
        .map_err(|error| format!("hook `{}` failed: {error}", hook.command))
}

pub(crate) async fn run(hook: &Hook, call: &Call<'_>) -> Verdict {
    let body = match hook.dialect {
        Dialect::Summonr => call.payload.clone(),
        Dialect::Claude => hook.claude_payload(call),
    };
    match execute(hook, call, &body).await {
        Ok(outcome) => judge(hook.dialect, call.event, &outcome),
        Err(error) => failed(hook.dialect, call.event, error),
    }
}
