use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::durable::store;
use crate::error::{Error, Result};

const FILE: &str = "jobs.toml";
const TOKEN_PREFIX: &str = "cron:";
const TOKEN_BYTES: usize = 32;
const PAYLOAD_SUFFIX: &str = "body";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Task {
    Prompt(String),
    Goal(String),
}

impl Default for Task {
    fn default() -> Self {
        Self::Prompt(String::new())
    }
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    pub chat: bool,
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

impl Notify {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        [Self::Never, Self::Failure, Self::Always]
            .into_iter()
            .find(|notify| name(notify) == text)
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

pub(crate) fn run_lock(name: &str) -> Result<PathBuf> {
    Ok(dir()?.join("runs").join(name))
}

pub(crate) fn remove_run_lock(name: &str) -> Result<()> {
    remove_quiet(&store::lock_path(&run_lock(name)?))
}

pub(crate) fn payload_path(workspace: &Path, name: &str, slot: i64) -> PathBuf {
    crate::jobs::dir(workspace).join(format!("{name}-{slot}.{PAYLOAD_SUFFIX}"))
}

pub(crate) fn remove_payloads(workspace: &Path, name: &str) -> Result<()> {
    let entries = match std::fs::read_dir(crate::jobs::dir(workspace)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let path = entry?.path();
        if payload_of(&path, name) {
            remove_quiet(&path)?;
        }
    }
    Ok(())
}

fn payload_of(path: &Path, name: &str) -> bool {
    path.extension().is_some_and(|ext| ext == PAYLOAD_SUFFIX)
        && path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.strip_prefix(name))
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(|slot| slot.parse::<i64>().is_ok())
}

fn remove_quiet(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        Ok(()) | Err(_) => Ok(()),
    }
}

fn trim(jobs: &mut [Job]) -> Result<()> {
    let kept = crate::limits::limits().cron_runs_kept;
    for job in jobs {
        let extra = job.runs.len().saturating_sub(kept);
        for run in job.runs.drain(..extra) {
            remove_quiet(&payload_path(&job.dir, &job.name, run.slot))?;
        }
    }
    Ok(())
}

pub(crate) fn check_task(task: &Task) -> Result<()> {
    match task {
        Task::Goal(goal) if goal.starts_with('-') => {
            Err(Error::Config("the goal cannot start with `-`".to_string()))
        }
        Task::Goal(_) | Task::Prompt(_) => Ok(()),
    }
}

pub(crate) fn rotate_token(name: &str) -> Result<String> {
    use base64::Engine;
    use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
    get(name)?;
    let mut bytes = [0u8; TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    crate::secret::set(&token_key(name), &token)?;
    Ok(token)
}

pub(crate) fn token(name: &str) -> Option<String> {
    crate::secret::get(&token_key(name))
        .ok()
        .flatten()
        .map(|(token, _)| token)
}

pub(crate) fn forget_token(name: &str) -> Result<bool> {
    crate::secret::forget(&token_key(name))
}

pub(crate) fn same_secret(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

fn token_key(name: &str) -> String {
    format!("{TOKEN_PREFIX}{name}")
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
    trim(&mut jobs)?;
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

pub(super) fn unknown(name: &str) -> Error {
    Error::Config(format!("no cron job named `{name}`"))
}
