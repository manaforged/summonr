use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::endpoint::{Endpoint, Requirement};

mod shape;

pub use shape::Shape;

const PLANS: &str = include_str!("quota/plans.toml");

/// The `[usage]` table in config.toml: plans to skip and plans to add.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct Settings {
    /// Plan names to skip.
    #[serde(default)]
    pub off: Vec<String>,
    /// Extra plans, read after the shipped ones.
    #[serde(default)]
    pub plan: Vec<Plan>,
}

/// Where one plan's usage comes from and how to read the reply.
#[derive(Deserialize, Clone, Debug)]
pub struct Plan {
    /// The name shown for the plan.
    pub name: String,
    #[serde(default)]
    os: Option<Os>,
    #[serde(default)]
    installed: Option<String>,
    #[serde(default)]
    via: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    token: Option<Field>,
    #[serde(default)]
    headers: BTreeMap<String, Field>,
    #[serde(default)]
    command: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    shape: Shape,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Os {
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

/// A value given as text, or read by JSON pointer from a file or a command's output.
#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
enum Field {
    Text(String),
    File { file: String, pointer: String },
    Command { command: Vec<String>, pointer: String },
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
            Self::Command { command, pointer } => (output(command, &BTreeMap::new()).await?, pointer),
        };
        let value: Value = serde_json::from_str(json.trim()).map_err(|_| "credential is not JSON".to_string())?;
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("no {pointer} in the credential"))
    }
}

/// One usage window of a plan: the share used and when it resets.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Window {
    /// The model group the window covers, when the plan has several.
    #[serde(default)]
    pub group: Option<String>,
    /// The window length in minutes, when the provider reports it.
    #[serde(default)]
    pub minutes: Option<u64>,
    /// The share used, from 0 to 100.
    pub used: f64,
    /// When the window resets, as an RFC 3339 time.
    #[serde(default)]
    pub resets: Option<String>,
}

impl Window {
    /// A short name for the window: 5-hour, week, or its length.
    #[must_use]
    pub fn label(&self) -> String {
        shape::label(self.minutes)
    }
}

/// A plan's windows, or why they could not be read.
#[derive(Serialize, Clone, Debug)]
pub struct Reading {
    /// The plan name.
    pub plan: String,
    /// The Summonr endpoint whose login read it, if any.
    pub endpoint: Option<String>,
    /// The windows read.
    pub windows: Vec<Window>,
    /// Why the windows could not be read.
    pub error: Option<String>,
}

/// Reads every plan that applies here: shipped plans and `[usage]` plans, minus `[usage] off`.
pub async fn read() -> Vec<Reading> {
    let settings = crate::config::file().map(|file| file.usage.clone()).unwrap_or_default();
    let shipped = toml::from_str::<Settings>(PLANS).map(|plans| plans.plan).unwrap_or_default();
    let endpoints: Vec<Endpoint> = crate::endpoint::set_up()
        .iter()
        .filter_map(|name| crate::endpoint::resolve(Some(name), None, Requirement::ModelAndKey).ok())
        .collect();
    let plans: Vec<Plan> = shipped
        .into_iter()
        .chain(settings.plan)
        .filter(|plan| !settings.off.contains(&plan.name))
        .collect();
    let jobs = plans.iter().filter_map(|plan| {
        let source = source(plan, &endpoints)?;
        Some(fetch(plan, source))
    });
    futures::future::join_all(jobs).await
}

enum Source<'a> {
    Login(&'a Endpoint, &'a str),
    Direct(&'a str),
    Command,
}

fn source<'a>(plan: &'a Plan, endpoints: &'a [Endpoint]) -> Option<Source<'a>> {
    if plan.os.is_some_and(|os| !os.current()) {
        return None;
    }
    let tool = plan.installed.as_deref().or(plan.command.first().map(String::as_str));
    if tool.is_some_and(|tool| crate::askpass::on_path(tool).is_none()) {
        return None;
    }
    if !plan.command.is_empty() {
        return Some(Source::Command);
    }
    let url = plan.url.as_deref()?;
    let Some(via) = plan.via.as_deref() else {
        return plan.token.is_some().then_some(Source::Direct(url));
    };
    endpoints
        .iter()
        .find(|endpoint| {
            let token_url = endpoint.oauth.as_ref().map(|oauth| oauth.token_url.as_str());
            [endpoint.base_url.as_deref(), token_url]
                .into_iter()
                .flatten()
                .any(|address| address.starts_with(via))
        })
        .map(|endpoint| Source::Login(endpoint, url))
}

async fn fetch(plan: &Plan, source: Source<'_>) -> Reading {
    let (endpoint, body) = match source {
        Source::Login(endpoint, url) => (Some(endpoint.name.clone()), login(endpoint, url).await),
        Source::Direct(url) => (None, direct(plan, url).await),
        Source::Command => (None, json(output(&plan.command, &plan.env).await)),
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
    }
}

async fn login(endpoint: &Endpoint, url: &str) -> Result<Value, String> {
    let client = crate::provider::Client::connect(endpoint).map_err(|error| error.to_string())?;
    client.get(url.to_string()).await.map_err(|error| error.to_string())
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
    let (program, args) = command.split_first().ok_or("`command` is empty")?;
    let mut spec = crate::process::Spec::new(program, args);
    spec.env = env.iter().map(|(key, value)| (key.into(), value.into())).collect();
    let process = crate::process::start(spec).map_err(|error| format!("did not start: {error}"))?;
    crate::panel::output(process, None).await
}

fn json(printed: Result<String, String>) -> Result<Value, String> {
    serde_json::from_str(&printed?).map_err(|error| format!("printed JSON that does not parse: {error}"))
}
