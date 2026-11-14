use std::collections::BTreeMap;

use super::config::Helper;
use super::scope::Scope;
use crate::process::{Cap, Env};
use summonr_core::error::{Error, Result};

const SERVER_NAME: &str = "CLAUDE_CODE_MCP_SERVER_NAME";
const SERVER_URL: &str = "CLAUDE_CODE_MCP_SERVER_URL";
const PLUGIN_ROOT: &str = "CLAUDE_PLUGIN_ROOT";

/// `headers` with the helper's printed headers merged over any of the same name.
pub(super) async fn headers(
    helper: &Helper,
    name: &str,
    url: &str,
    headers: &[(String, String)],
) -> Result<Vec<(String, String)>> {
    let origin = &helper.origin;
    let mut spec = crate::process::shell(&helper.command, &origin.dir)?
        .envs([(SERVER_NAME, name), (SERVER_URL, url)]);
    match origin.scope {
        Scope::Plugin => spec = spec.envs([(PLUGIN_ROOT, origin.dir.as_os_str())]),
        Scope::Project => {}
        Scope::Local | Scope::User => spec.base = Env::Inherit,
    }
    let what = format!("mcp {name} headersHelper");
    let limits = crate::limits::limits();
    let max = limits.hook_output_max_bytes;
    let ran = crate::process::finish(
        crate::process::start(spec)?,
        &[],
        (Cap::Head(max), Cap::Head(max)),
        limits.mcp_headers_helper(),
        what.clone(),
    )
    .await??
    .trimmed();
    if ran.code != Some(0) {
        return Err(Error::Config(format!(
            "{what} {}: {}",
            crate::process::exited(ran.code),
            ran.stderr
        )));
    }
    let printed: BTreeMap<String, String> = serde_json::from_str(&ran.stdout).map_err(|error| {
        Error::Config(format!(
            "{what} must print a JSON object of strings: {error}"
        ))
    })?;
    let mut merged: Vec<(String, String)> = headers
        .iter()
        .filter(|(key, _)| !printed.keys().any(|top| top.eq_ignore_ascii_case(key)))
        .cloned()
        .collect();
    merged.extend(printed);
    Ok(merged)
}
