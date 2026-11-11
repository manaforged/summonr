use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::endpoint::{Endpoint, Requirement};

mod fetch;
mod refresh;
mod shape;

pub use shape::Shape;

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

fn shipped() -> &'static Shipped {
    static SHIPPED: OnceLock<Shipped> = OnceLock::new();
    SHIPPED.get_or_init(|| summonr_core::embedded!("usage plan", "quota/plans.toml"))
}

fn settings() -> Settings {
    crate::config::file()
        .map(|file| file.usage.clone())
        .unwrap_or_default()
}

/// Where one plan's usage comes from and how to read the reply.
#[derive(Deserialize, Clone, Debug)]
pub struct Plan {
    /// The name shown for the plan.
    pub name: String,
    #[serde(default)]
    os: Option<fetch::Os>,
    #[serde(default)]
    installed: Option<String>,
    #[serde(default)]
    via: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    token: Option<fetch::Field>,
    #[serde(default)]
    headers: BTreeMap<String, fetch::Field>,
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
        Some(shape::countdown(
            at.duration_since(jiff::Timestamp::now()).as_secs(),
        ))
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

pub(crate) fn remember(name: &str, reported: Option<&Value>) -> std::io::Result<()> {
    let windows = reported.map(shape::unified).unwrap_or_default();
    if windows.is_empty() {
        return Ok(());
    }
    refresh::write_cache(&format!("{name}.json"), &windows)
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
    let endpoints: Vec<Endpoint> = crate::endpoint::set_up()
        .iter()
        .filter_map(|name| {
            crate::endpoint::resolve(Some(name), None, Requirement::ModelAndKey).ok()
        })
        .collect();
    let plans: Vec<Plan> = shipped()
        .plan
        .iter()
        .cloned()
        .chain(settings.plan)
        .filter(|plan| !settings.off.contains(&plan.name))
        .collect();
    let jobs = plans.iter().filter_map(|plan| {
        let source = fetch::source(plan, &endpoints)?;
        Some(fetch::fetch(plan, source))
    });
    futures::future::join_all(jobs).await
}
