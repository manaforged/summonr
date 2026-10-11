use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::durable::store;
use crate::error::{Error, Result};

const FILE: &str = "jobs.toml";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Task {
    Prompt(String),
    Goal(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Notify {
    Never,
    #[default]
    Failure,
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    Running,
    Done,
    CheckFailed,
    Unfinished,
    Error,
    Overlap,
    Expired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Run {
    pub slot: i64,
    pub started: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Job {
    pub name: String,
    pub schedule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tz: Option<String>,
    pub task: Task,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub dir: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub worktree: bool,
    #[serde(default)]
    pub notify: Notify,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mail: Option<String>,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub fails: u32,
    pub created: i64,
    #[serde(default)]
    pub runs: Vec<Run>,
}

#[derive(Default, Serialize, Deserialize)]
struct Book {
    #[serde(default)]
    job: Vec<Job>,
}

impl Outcome {
    pub(crate) fn from_code(code: Option<i32>) -> Self {
        match code {
            Some(0) => Self::Done,
            Some(2) => Self::CheckFailed,
            Some(3) => Self::Unfinished,
            Some(_) | None => Self::Error,
        }
    }

    pub(crate) fn failed(self) -> bool {
        matches!(self, Self::CheckFailed | Self::Unfinished | Self::Error)
    }
}

impl Job {
    pub(crate) fn last_slot(&self) -> Option<i64> {
        self.runs.iter().map(|run| run.slot).max()
    }
}

pub(crate) fn name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

pub(crate) fn dir() -> Result<PathBuf> {
    Ok(crate::prompt::summonr_home()?.join("cron"))
}

pub(crate) fn check_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_');
    if valid {
        Ok(())
    } else {
        Err(Error::Config(format!(
            "bad job name `{name}`: use a-z, 0-9, - and _"
        )))
    }
}

pub(crate) fn load() -> Result<Vec<Job>> {
    let path = dir()?.join(FILE);
    let _guard = store::lock(&path, true)?;
    read(&path)
}

pub(crate) fn get(name: &str) -> Result<Job> {
    load()?
        .into_iter()
        .find(|job| job.name == name)
        .ok_or_else(|| unknown(name))
}

pub(crate) fn update<T>(change: impl FnOnce(&mut Vec<Job>) -> Result<T>) -> Result<T> {
    let path = dir()?.join(FILE);
    let _guard = store::lock(&path, true)?;
    let mut jobs = read(&path)?;
    let out = change(&mut jobs)?;
    let kept = crate::limits::limits().cron_runs_kept;
    for job in &mut jobs {
        let extra = job.runs.len().saturating_sub(kept);
        job.runs.drain(..extra);
    }
    let text = toml::to_string(&Book { job: jobs })
        .map_err(|error| Error::Config(format!("{}: {error}", path.display())))?;
    store::write(&path, text)?;
    Ok(out)
}

pub(crate) fn with_job<T>(name: &str, change: impl FnOnce(&mut Job) -> Result<T>) -> Result<T> {
    update(|jobs| {
        let job = jobs
            .iter_mut()
            .find(|job| job.name == name)
            .ok_or_else(|| unknown(name))?;
        change(job)
    })
}

pub(crate) fn settle(
    name: &str,
    slot: i64,
    started: i64,
    change: impl FnOnce(&mut Run),
) -> Result<()> {
    with_job(name, |job| {
        if !job.runs.iter().any(|run| run.slot == slot) {
            job.runs.push(Run {
                slot,
                started,
                job: None,
                outcome: Outcome::Running,
            });
        }
        if let Some(run) = job.runs.iter_mut().find(|run| run.slot == slot) {
            change(run);
        }
        Ok(())
    })
}

fn read(path: &Path) -> Result<Vec<Job>> {
    let text = store::read_or_empty(path)?;
    toml::from_str::<Book>(&text)
        .map(|book| book.job)
        .map_err(|error| Error::Config(format!("{}: {error}", path.display())))
}

fn unknown(name: &str) -> Error {
    Error::Config(format!("no cron job named `{name}`"))
}
