use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::{Plan, Reading};
use crate::endpoint::Endpoint;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Os {
    Macos,
    Linux,
    Windows,
}

impl Os {
    fn current(self) -> bool {
        match self {
            Self::Macos => cfg!(target_os = "macos"),
            Self::Linux => cfg!(target_os = "linux"),
            Self::Windows => cfg!(target_os = "windows"),
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub(super) enum Field {
    Text(String),
    File {
        file: String,
        pointer: String,
    },
    Command {
        command: Vec<String>,
        pointer: String,
    },
}

impl Field {
    async fn value(&self) -> Result<String, String> {
        let (json, pointer) = match self {
            Self::Text(text) => return Ok(text.clone()),
            Self::File { file, pointer } => {
                let path = crate::config::expand_tilde(file).unwrap_or_else(|| file.into());
                let text = std::fs::read_to_string(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                (text, pointer)
            }
            Self::Command { command, pointer } => {
                (output(command, &BTreeMap::new()).await?, pointer)
            }
        };
        let value: Value = serde_json::from_str(json.trim())
            .map_err(|_error| "credential is not JSON".to_string())?;
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("no {pointer} in the credential"))
    }
}

pub(super) enum Source<'a> {
    Login(&'a Endpoint, &'a str),
    Direct(&'a str),
    Command,
    File(&'a str),
}

pub(super) fn source<'a>(plan: &'a Plan, endpoints: &'a [Endpoint]) -> Option<Source<'a>> {
    if plan.os.is_some_and(|os| !os.current()) {
        return None;
    }
    let tool = plan
        .installed
        .as_deref()
        .or(plan.command.first().map(String::as_str));
    if tool.is_some_and(|tool| crate::askpass::on_path(tool).is_none()) {
        return None;
    }
    if !plan.command.is_empty() {
        return Some(Source::Command);
    }
    if let Some(file) = plan.file.as_deref() {
        return path(file).exists().then_some(Source::File(file));
    }
    let url = plan.url.as_deref()?;
    let Some(via) = plan.via.as_deref() else {
        return plan.token.is_some().then_some(Source::Direct(url));
    };
    endpoints
        .iter()
        .find(|endpoint| {
            let token_url = endpoint
                .oauth
                .as_ref()
                .map(|oauth| oauth.token_url.as_str());
            [endpoint.base_url.as_deref(), token_url]
                .into_iter()
                .flatten()
                .any(|address| address.starts_with(via))
        })
        .map(|endpoint| Source::Login(endpoint, url))
}

pub(super) async fn fetch(plan: &Plan, source: Source<'_>) -> Reading {
    let at = match source {
        Source::File(file) => modified(&path(file)),
        Source::Login(..) | Source::Direct(_) | Source::Command => {
            Some(jiff::Timestamp::now().to_string())
        }
    };
    let (endpoint, body) = match source {
        Source::Login(endpoint, url) => (Some(endpoint.name.clone()), login(endpoint, url).await),
        Source::Direct(url) => (None, direct(plan, url).await),
        Source::Command if plan.reply.is_some() => (None, exchange(plan).await),
        Source::Command => (None, json(output(&plan.command, &plan.env).await)),
        Source::File(file) => (
            None,
            json(std::fs::read_to_string(path(file)).map_err(|error| error.to_string())),
        ),
    };
    let (windows, error) = match body.and_then(|value| plan.shape.parse(&value)) {
        Ok(windows) => (windows, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    Reading {
        plan: plan.name.clone(),
        endpoint,
        windows,
        error,
        at,
    }
}

fn path(file: &str) -> PathBuf {
    let mut file = file.to_string();
    if let Ok(cache) = super::refresh::folder() {
        file = file.replace("$QUOTA_CACHE", &cache.to_string_lossy());
    }
    if let Ok(home) = summonr_core::dirs::summonr_home() {
        file = file.replace("$SUMMONR_HOME", &home.to_string_lossy());
    }
    crate::config::expand_tilde(&file).unwrap_or_else(|| file.into())
}

fn modified(path: &Path) -> Option<String> {
    let time = std::fs::metadata(path).ok()?.modified().ok()?;
    jiff::Timestamp::try_from(time)
        .ok()
        .map(|time| time.to_string())
}

fn start_command(
    command: &[String],
    env: &BTreeMap<String, String>,
) -> Result<crate::process::Process, String> {
    let (program, args) = command.split_first().ok_or("`command` is empty")?;
    let spec = crate::process::Spec::new(program, args).envs(env);
    crate::process::start(spec).map_err(|error| format!("did not start: {error}"))
}

async fn exchange(plan: &Plan) -> Result<Value, String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let reply = plan.reply.as_deref().unwrap_or_default();
    let mut process = start_command(&plan.command, &plan.env)?;
    for line in &plan.stdin {
        process
            .stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|error| error.to_string())?;
    }
    let mut lines = tokio::io::BufReader::new(&mut process.stdout).lines();
    let answer = async {
        while let Some(line) = lines.next_line().await.map_err(|error| error.to_string())? {
            let value: Option<Value> = serde_json::from_str(&line).ok();
            if let Some(found) = value.as_ref().and_then(|value| value.pointer(reply)) {
                return Ok(found.clone());
            }
        }
        Err(format!("exited before printing {reply}"))
    };
    crate::limits::within(
        crate::limits::limits().panel_timeout(),
        format!("waiting for {reply}"),
        answer,
    )
    .await
    .unwrap_or_else(|error| Err(error.to_string()))
}

async fn login(endpoint: &Endpoint, url: &str) -> Result<Value, String> {
    let client = crate::provider::Client::connect(endpoint).map_err(|error| error.to_string())?;
    client
        .get(url.to_string())
        .await
        .map_err(|error| error.to_string())
}

async fn direct(plan: &Plan, url: &str) -> Result<Value, String> {
    let token = match &plan.token {
        Some(field) => field.value().await?,
        None => return Err("no token".to_string()),
    };
    let http = crate::credentials::http(crate::limits::limits().model_probe())
        .map_err(|error| error.to_string())?;
    let mut request = http.get(url).bearer_auth(&token);
    for (name, field) in &plan.headers {
        request = request.header(name.as_str(), field.value().await?.as_str());
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{url} answered {status}"));
    }
    response.json().await.map_err(|error| error.to_string())
}

async fn output(command: &[String], env: &BTreeMap<String, String>) -> Result<String, String> {
    let process = start_command(command, env)?;
    crate::panel::output(process, None).await
}

fn json(printed: Result<String, String>) -> Result<Value, String> {
    serde_json::from_str(&printed?)
        .map_err(|error| format!("printed JSON that does not parse: {error}"))
}

#[cfg(test)]
#[path = "fetch_tests.rs"]
mod tests;
