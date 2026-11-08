use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::endpoint::{Endpoint, Requirement};

mod refresh;
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
    /// Seconds between reads, 1800 by default; 0 reads only on `endpoint usage`.
    #[serde(default)]
    pub every_secs: Option<u64>,
}

#[derive(Deserialize, Default)]
struct Shipped {
    #[serde(default)]
    plan: Vec<Plan>,
    #[serde(default)]
    every_secs: u64,
}

fn shipped() -> Shipped {
    toml::from_str(PLANS).unwrap_or_default()
}

fn settings() -> Settings {
    crate::config::file().map(|file| file.usage.clone()).unwrap_or_default()
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
    #[serde(default)]
    stdin: Vec<String>,
    #[serde(default)]
    reply: Option<String>,
    #[serde(default)]
    file: Option<String>,
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

    /// The time left before the window resets, as `Resets in 2d 4h`, `Resets in 45m` or `Resets now`.
    #[must_use]
    pub fn resets_in(&self) -> Option<String> {
        let at: jiff::Timestamp = self.resets.as_deref()?.parse().ok()?;
        Some(shape::countdown(at.duration_since(jiff::Timestamp::now()).as_secs()))
    }
}

impl Reading {
    /// How old the windows are, as `Updated just now`, `Updated 5m ago` or `Updated 14:05`.
    #[must_use]
    pub fn updated(&self) -> Option<String> {
        let at: jiff::Timestamp = self.at.as_deref()?.parse().ok()?;
        Some(shape::updated(at, jiff::Timestamp::now()))
    }
}

/// A plan's windows, or why they could not be read.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Reading {
    /// The plan name.
    pub plan: String,
    /// The Summonr endpoint whose login read it, if any.
    pub endpoint: Option<String>,
    /// The windows read.
    pub windows: Vec<Window>,
    /// Why the windows could not be read.
    pub error: Option<String>,
    /// When the windows were read, as an RFC 3339 time.
    pub at: Option<String>,
}

/// Records windows a running tool reported, for the plan that reads `cache/quota/NAME.json`.
pub(crate) fn remember(name: &str, reported: Option<&Value>) -> std::io::Result<()> {
    let windows = reported.map(shape::unified).unwrap_or_default();
    if windows.is_empty() {
        return Ok(());
    }
    let home = summonr_core::dirs::summonr_home().map_err(std::io::Error::other)?;
    let folder = home.join("cache/quota");
    std::fs::create_dir_all(&folder)?;
    let bytes = serde_json::to_vec(&windows).map_err(std::io::Error::other)?;
    crate::durable::store::write(&folder.join(format!("{name}.json")), bytes, crate::durable::store::Durability::Renamed)
}

/// The saved readings while younger than the cadence, else a new read.
pub async fn latest() -> Vec<Reading> {
    refresh::cached(cadence(), read).await
}

/// Reads every plan now and saves the readings for `latest`.
pub async fn refresh() -> Vec<Reading> {
    refresh::cached(Some(std::time::Duration::ZERO), read).await
}

/// How often a view that stays open calls `refresh`; `None` when refresh is manual.
#[must_use]
pub fn cadence() -> Option<std::time::Duration> {
    let secs = settings().every_secs.unwrap_or(shipped().every_secs);
    (secs > 0).then(|| std::time::Duration::from_secs(secs))
}

async fn read() -> Vec<Reading> {
    let settings = settings();
    let shipped = shipped().plan;
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
    File(&'a str),
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
            let token_url = endpoint.oauth.as_ref().map(|oauth| oauth.token_url.as_str());
            [endpoint.base_url.as_deref(), token_url]
                .into_iter()
                .flatten()
                .any(|address| address.starts_with(via))
        })
        .map(|endpoint| Source::Login(endpoint, url))
}

async fn fetch(plan: &Plan, source: Source<'_>) -> Reading {
    let at = match source {
        Source::File(file) => modified(&path(file)),
        _ => Some(jiff::Timestamp::now().to_string()),
    };
    let (endpoint, body) = match source {
        Source::Login(endpoint, url) => (Some(endpoint.name.clone()), login(endpoint, url).await),
        Source::Direct(url) => (None, direct(plan, url).await),
        Source::Command if plan.reply.is_some() => (None, exchange(plan).await),
        Source::Command => (None, json(output(&plan.command, &plan.env).await)),
        Source::File(file) => (None, json(std::fs::read_to_string(path(file)).map_err(|error| error.to_string()))),
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

fn path(file: &str) -> std::path::PathBuf {
    let file = match summonr_core::dirs::summonr_home() {
        Ok(home) => file.replace("$SUMMONR_HOME", &home.to_string_lossy()),
        Err(_) => file.to_string(),
    };
    crate::config::expand_tilde(&file).unwrap_or_else(|| file.into())
}

fn modified(path: &std::path::Path) -> Option<String> {
    let time = std::fs::metadata(path).ok()?.modified().ok()?;
    jiff::Timestamp::try_from(time).ok().map(|time| time.to_string())
}

async fn exchange(plan: &Plan) -> Result<Value, String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let reply = plan.reply.as_deref().unwrap_or_default();
    let (program, args) = plan.command.split_first().ok_or("`command` is empty")?;
    let mut spec = crate::process::Spec::new(program, args);
    spec.env = plan.env.iter().map(|(key, value)| (key.into(), value.into())).collect();
    let mut process = crate::process::start(spec).map_err(|error| format!("did not start: {error}"))?;
    for line in &plan.stdin {
        process.stdin.write_all(format!("{line}\n").as_bytes()).await.map_err(|error| error.to_string())?;
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
    tokio::time::timeout(crate::limits::limits().panel_timeout(), answer)
        .await
        .map_err(|_| format!("no {reply} within {}s", crate::limits::limits().panel_timeout_secs))?
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
