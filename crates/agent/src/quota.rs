use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::endpoint::{Endpoint, Requirement};

const PLANS: &str = include_str!("quota/plans.toml");
const FIVE_HOURS: u64 = 300;
const WEEK: u64 = 10_080;

#[derive(Deserialize)]
struct Plans {
    plan: Vec<Plan>,
}

#[derive(Deserialize)]
struct Plan {
    name: String,
    #[serde(default)]
    via: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    command: Vec<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    shape: Shape,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Shape {
    Wham,
    Zai,
    Antigravity,
}

/// One usage window of a plan: the share used and when it resets.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Window {
    /// The model group the window covers, when the plan has several.
    pub group: Option<String>,
    /// The window length in minutes, when the provider reports it.
    pub minutes: Option<u64>,
    /// The share used, from 0 to 100.
    pub used: f64,
    /// When the window resets, as an RFC 3339 time.
    pub resets: Option<String>,
}

impl Window {
    /// A short name for the window: 5-hour, week, or its length.
    #[must_use]
    pub fn label(&self) -> String {
        match self.minutes {
            Some(FIVE_HOURS) => "5-hour".to_string(),
            Some(WEEK) => "week".to_string(),
            Some(minutes) if minutes % 1440 == 0 => format!("{} d", minutes / 1440),
            Some(minutes) if minutes % 60 == 0 => format!("{} h", minutes / 60),
            Some(minutes) => format!("{minutes} min"),
            None => "window".to_string(),
        }
    }
}

/// A plan's windows, or why they could not be read.
#[derive(Serialize, Clone, Debug)]
pub struct Reading {
    /// The plan name.
    pub plan: String,
    /// The endpoint whose login read it, if any.
    pub endpoint: Option<String>,
    /// The windows read.
    pub windows: Vec<Window>,
    /// Why the windows could not be read.
    pub error: Option<String>,
}

/// Reads every plan that has a set-up endpoint or an installed command.
pub async fn read() -> Vec<Reading> {
    let Ok(plans) = toml::from_str::<Plans>(PLANS) else {
        return Vec::new();
    };
    let endpoints: Vec<Endpoint> = crate::endpoint::set_up()
        .iter()
        .filter_map(|name| {
            crate::endpoint::resolve(Some(name), None, Requirement::ModelAndKey).ok()
        })
        .collect();
    let jobs = plans.plan.iter().filter_map(|plan| {
        let source = source(plan, &endpoints)?;
        Some(fetch(plan, source))
    });
    futures::future::join_all(jobs).await
}

enum Source<'a> {
    Http(&'a Endpoint, &'a str),
    Command(&'a [String]),
}

fn source<'a>(plan: &'a Plan, endpoints: &'a [Endpoint]) -> Option<Source<'a>> {
    if let Some(program) = plan.command.first() {
        return crate::askpass::on_path(program).map(|_| Source::Command(&plan.command));
    }
    let (via, url) = (plan.via.as_deref()?, plan.url.as_deref()?);
    endpoints
        .iter()
        .find(|endpoint| {
            let token_url = endpoint.oauth.as_ref().map(|oauth| oauth.token_url.as_str());
            [endpoint.base_url.as_deref(), token_url]
                .into_iter()
                .flatten()
                .any(|address| address.starts_with(via))
        })
        .map(|endpoint| Source::Http(endpoint, url))
}

async fn fetch(plan: &Plan, source: Source<'_>) -> Reading {
    let (endpoint, body) = match source {
        Source::Http(endpoint, url) => (Some(endpoint.name.clone()), get(endpoint, url).await),
        Source::Command(command) => (None, run(command, &plan.env).await),
    };
    let (windows, error) = match body.and_then(|value| parse(plan.shape, &value)) {
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

async fn get(endpoint: &Endpoint, url: &str) -> Result<Value, String> {
    let client = crate::provider::Client::connect(endpoint).map_err(|error| error.to_string())?;
    client.get(url.to_string()).await.map_err(|error| error.to_string())
}

async fn run(command: &[String], env: &std::collections::BTreeMap<String, String>) -> Result<Value, String> {
    let (program, args) = command.split_first().ok_or("`command` is empty")?;
    let mut spec = crate::process::Spec::new(program, args);
    spec.env = env.iter().map(|(key, value)| (key.into(), value.into())).collect();
    let process = crate::process::start(spec).map_err(|error| format!("did not start: {error}"))?;
    let printed = crate::panel::output(process, None).await?;
    serde_json::from_str(&printed).map_err(|error| format!("printed JSON that does not parse: {error}"))
}

fn parse(shape: Shape, value: &Value) -> Result<Vec<Window>, String> {
    let windows = match shape {
        Shape::Wham => wham(value),
        Shape::Zai => zai(value),
        Shape::Antigravity => antigravity(value),
    };
    if windows.is_empty() {
        return Err(format!("no usage windows in the {shape:?} reply"));
    }
    Ok(windows)
}

fn wham(value: &Value) -> Vec<Window> {
    let limits = &value["rate_limit"];
    ["primary_window", "secondary_window"]
        .iter()
        .filter_map(|key| {
            let window = limits.get(key)?;
            Some(Window {
                group: None,
                minutes: window["limit_window_seconds"].as_u64().map(|seconds| seconds / 60),
                used: window["used_percent"].as_f64()?,
                resets: window["reset_at"].as_i64().and_then(|at| time(at, 1)),
            })
        })
        .collect()
}

fn zai(value: &Value) -> Vec<Window> {
    let Some(limits) = value["data"]["limits"].as_array() else {
        return Vec::new();
    };
    limits
        .iter()
        .filter(|limit| matches!(limit["type"].as_str(), Some("TOKENS_LIMIT" | "CREDIT_LIMIT")))
        .filter_map(|limit| {
            let unit = match limit["unit"].as_u64()? {
                1 => 1440,
                3 => 60,
                5 => 1,
                6 => WEEK,
                _ => return None,
            };
            Some(Window {
                group: None,
                minutes: limit["number"].as_u64().map(|number| number * unit),
                used: limit["percentage"].as_f64()?,
                resets: limit["nextResetTime"].as_i64().and_then(|at| time(at, 1000)),
            })
        })
        .collect()
}

fn antigravity(value: &Value) -> Vec<Window> {
    let Some(groups) = value["command"]["data"]["groups"].as_array() else {
        return Vec::new();
    };
    groups
        .iter()
        .flat_map(|group| {
            let name = group["name"].as_str().map(str::to_string);
            group["buckets"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|bucket| !bucket["disabled"].as_bool().unwrap_or(false))
                .filter_map(move |bucket| {
                    Some(Window {
                        group: name.clone(),
                        minutes: match bucket["window"].as_str()? {
                            "5h" => Some(FIVE_HOURS),
                            "weekly" => Some(WEEK),
                            _ => None,
                        },
                        used: (1.0 - bucket["remaining_fraction"].as_f64()?) * 100.0,
                        resets: bucket["reset_time"].as_str().map(str::to_string),
                    })
                })
        })
        .collect()
}

fn time(at: i64, per_second: i64) -> Option<String> {
    jiff::Timestamp::from_second(at / per_second)
        .ok()
        .map(|time| time.to_string())
}
