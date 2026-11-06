use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::durable::store;
use summonr_core::error::{Error, Result};

const ID_MAX: usize = 80;
const KILLED: &str = "killed";
const ENDED: &str = "ended";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub pid: u32,
    pub command: String,
    pub workspace: String,
    pub started: u64,
    pub background: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub detached: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Running,
    Killed,
    Exited(i32),
    Dead,
}

impl std::fmt::Display for JobState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => f.write_str("running"),
            Self::Killed => f.write_str(KILLED),
            Self::Exited(code) => write!(f, "exit {code}"),
            Self::Dead => f.write_str("dead"),
        }
    }
}

#[derive(Clone)]
pub struct Status {
    pub text: String,
    pub state: JobState,
}

impl Status {
    pub fn label(&self) -> String {
        self.state.to_string()
    }
}

pub(crate) fn prepare(workspace: &Path) -> Result<(String, PathBuf, PathBuf, PathBuf)> {
    let root = place::made(workspace)?;
    let ignore = root.join(".gitignore");
    if !ignore.exists() {
        store::write(&ignore, "*\n", store::Durability::Renamed)?;
    }
    let id = new_id()?;
    Ok((
        id.clone(),
        root.join(format!("{id}.json")),
        root.join(format!("{id}.log")),
        root.join(format!("{id}.exit")),
    ))
}

static STARTED: std::sync::Mutex<Vec<Record>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn write(path: &Path, record: &Record) -> Result<()> {
    store::write(
        path,
        serde_json::to_string(record)?,
        store::Durability::Renamed,
    )?;
    if record.background {
        let mut started = STARTED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        started.retain(|known| known.id != record.id);
        started.push(record.clone());
    }
    Ok(())
}

/// Background jobs this process started, without reading the jobs folder.
pub(crate) fn started_here() -> Vec<Record> {
    STARTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

pub fn load(workspace: &Path, id: &str) -> Result<Record> {
    find(workspace, id)?.ok_or_else(|| Error::Tool(format!("unknown job `{id}`: no record")))
}

pub(crate) fn find(workspace: &Path, id: &str) -> Result<Option<Record>> {
    let id = check_id(id)?;
    let path = dir(workspace).join(format!("{id}.json"));
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::Tool(format!("unknown job `{id}`: {error}"))),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| Error::Tool(format!("job `{id}` is invalid: {error}")))
}

pub fn live(workspace: &Path) -> Vec<String> {
    since(workspace, 0, None)
        .into_iter()
        .filter(|record| {
            !dir(workspace).join(format!("{}.exit", record.id)).is_file()
                && alive_since(record.pid, record.started)
        })
        .map(|record| {
            format!(
                "{} `{}` (pid {}; stop it with run_command job kill, or delete {})",
                record.id,
                record.command,
                record.pid,
                dir(workspace).join(format!("{}.json", record.id)).display()
            )
        })
        .collect()
}

pub(crate) fn since(workspace: &Path, started: u64, session: Option<&str>) -> Vec<Record> {
    let root = dir(workspace);
    let mut jobs: Vec<Record> = summonr_core::walk::children(&root)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| {
            let text = std::fs::read_to_string(entry.path()).ok()?;
            serde_json::from_str::<Record>(&text).ok()
        })
        .filter(|record| !prune(workspace, record))
        .filter(|record| {
            record.background
                && record.started >= started
                && match (session, record.session.as_deref()) {
                    (Some(mine), Some(theirs)) => mine == theirs,
                    _ => true,
                }
        })
        .collect();
    jobs.sort_by(|left, right| {
        left.started
            .cmp(&right.started)
            .then_with(|| left.id.cmp(&right.id))
    });
    jobs
}

fn prune(workspace: &Path, record: &Record) -> bool {
    let cutoff =
        summonr_core::clock::now().saturating_sub(crate::limits::limits().job_retain().as_secs());
    let exit = exit_path(workspace, &record.id);
    if check_id(&record.id).is_err() || record.started >= cutoff || !finished(workspace, record) {
        return false;
    }
    let root = dir(workspace);
    for path in [
        root.join(format!("{}.json", record.id)),
        root.join(format!("{}.log", record.id)),
        exit,
    ] {
        drop(std::fs::remove_file(path));
    }
    true
}

pub(crate) fn finished(workspace: &Path, record: &Record) -> bool {
    exit_path(workspace, &record.id).is_file() || !alive_since(record.pid, record.started)
}

pub(crate) fn stamp(
    id: &str,
    pid: u32,
    command: &str,
    workspace: &Path,
    background: bool,
    session: Option<String>,
) -> Result<Record> {
    let started = summonr_core::clock::now();
    Ok(Record {
        id: id.to_string(),
        pid,
        command: command.to_string(),
        workspace: workspace.display().to_string(),
        started,
        background,
        session,
        log: None,
        detached: false,
    })
}

pub(crate) fn write_killed(path: &Path, children: &[String]) -> Result<()> {
    write_end(path, KILLED, children)
}

pub(crate) fn write_unowned(path: &Path, pid: u32) -> Result<()> {
    write_end(
        path,
        ENDED,
        &[format!(
            "pid {pid} was not signalled: it cannot be verified as this job (the pid may be reused); stop it by hand if it is yours"
        )],
    )
}

fn write_end(path: &Path, head: &str, lines: &[String]) -> Result<()> {
    let mut text = format!("{head}\n");
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    store::write(path, text, store::Durability::Renamed)?;
    Ok(())
}

pub(crate) fn exit_path(workspace: &Path, id: &str) -> PathBuf {
    dir(workspace).join(format!("{id}.exit"))
}

pub fn log_path(workspace: &Path, record: &Record) -> PathBuf {
    record.log.as_ref().map_or_else(
        || dir(workspace).join(format!("{}.log", record.id)),
        PathBuf::from,
    )
}

pub(crate) async fn wait(
    workspace: &Path,
    id: &str,
    limit_secs: u64,
    mut waiting: impl FnMut() -> Result<()>,
) -> Result<Status> {
    let started = std::time::Instant::now();
    let mut pause = crate::limits::limits().job_poll_start_secs;
    let mut announced = false;
    loop {
        let elapsed = started.elapsed().as_secs();
        let current = report(
            workspace,
            &load(workspace, id)?,
            (elapsed >= limit_secs).then_some(&std::cell::OnceCell::new()),
        )?;
        if current.state != JobState::Running || elapsed >= limit_secs {
            return Ok(current);
        }
        if !announced {
            announced = true;
            waiting()?;
        }
        tokio::time::sleep(crate::limits::seconds(pause.min(limit_secs - elapsed))).await;
        pause = (pause * 2).min(crate::limits::limits().job_poll_cap_secs);
    }
}

#[path = "jobs/owner.rs"]
mod owner;
pub(crate) use owner::{Owner, alive_since, owner};

#[path = "jobs/place.rs"]
mod place;
pub(crate) use place::dir;

#[path = "jobs/report.rs"]
mod report;
use report::report;
pub(crate) use report::{preview, status, statuses};

#[path = "jobs/tree.rs"]
mod tree;
pub(crate) use tree::{block, child_lines};

fn check_id(id: &str) -> Result<&str> {
    if id.is_empty() || id.len() > ID_MAX {
        return Err(Error::Tool("job id is invalid".to_string()));
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(Error::Tool("job id is invalid".to_string()));
    }
    Ok(id)
}

fn new_id() -> Result<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Config(error.to_string()))?
        .as_nanos();
    Ok(format!("{nanos}-{}", std::process::id()))
}

#[cfg(test)]
#[path = "jobs_tests.rs"]
mod tests;
