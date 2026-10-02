use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::durable::store;
use crate::error::{Error, Result};

const ID_MAX: usize = 80;
const KILLED: &str = "killed";
const POLL_START_SECS: u64 = 1;
const POLL_CAP_SECS: u64 = 10;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub id: String,
    pub pid: u32,
    pub command: String,
    pub workspace: String,
    pub started: u64,
    pub background: bool,
    #[serde(default)]
    pub sandbox: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
}

#[derive(Clone)]
pub(crate) struct Status {
    pub text: String,
    pub exit_code: Option<i32>,
    pub running: bool,
    pub killed: bool,
}

impl Status {
    pub(crate) fn label(&self) -> String {
        if self.running {
            return "running".to_string();
        }
        if self.killed {
            return "killed".to_string();
        }
        match self.exit_code {
            Some(code) => format!("exit {code}"),
            None => "dead".to_string(),
        }
    }
}

pub(crate) fn dir(workspace: &Path) -> PathBuf {
    workspace.join(".summonr").join("jobs")
}

pub(crate) fn prepare(workspace: &Path) -> Result<(String, PathBuf, PathBuf, PathBuf)> {
    let root = dir(workspace);
    std::fs::create_dir_all(&root)?;
    let ignore = root.join(".gitignore");
    if !ignore.exists() {
        store::write(&ignore, "*\n")?;
    }
    let id = new_id()?;
    Ok((
        id.clone(),
        root.join(format!("{id}.json")),
        root.join(format!("{id}.log")),
        root.join(format!("{id}.exit")),
    ))
}

pub(crate) fn write(path: &Path, record: &Record) -> Result<()> {
    store::write(path, serde_json::to_string(record)?)?;
    Ok(())
}

pub(crate) fn load(workspace: &Path, id: &str) -> Result<Record> {
    let id = check_id(id)?;
    let path = dir(workspace).join(format!("{id}.json"));
    let text = std::fs::read_to_string(&path)
        .map_err(|error| Error::Tool(format!("unknown job `{id}`: {error}")))?;
    serde_json::from_str(&text)
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
                "{} `{}` (pid {}; stop it with run_command job kill, or delete .summonr/jobs/{}.json)",
                record.id, record.command, record.pid, record.id
            )
        })
        .collect()
}

pub(crate) fn since(workspace: &Path, started: u64, session: Option<&str>) -> Vec<Record> {
    let root = dir(workspace);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut jobs: Vec<Record> = entries
        .flatten()
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
    jobs.sort_by_key(|record| record.started);
    jobs
}

fn prune(workspace: &Path, record: &Record) -> bool {
    let cutoff = crate::mail::now().saturating_sub(crate::limits::JOB_RETAIN.as_secs());
    let exit = exit_path(workspace, &record.id);
    if check_id(&record.id).is_err()
        || record.started >= cutoff
        || (!exit.is_file() && alive_since(record.pid, record.started))
    {
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

pub(crate) fn status(workspace: &Path, record: &Record) -> Result<Status> {
    let log = log_path(workspace, record);
    let exit = exit_path(workspace, &record.id);
    let bytes = std::fs::metadata(&log).map(|meta| meta.len()).unwrap_or(0);
    let mut killed = false;
    let alive = !exit.is_file() && alive_since(record.pid, record.started);
    let (state, exit_code, running) = if alive {
        ("running".to_string(), None, true)
    } else if exit.is_file() {
        let text = crate::durable::store::read_or_empty(&exit)?;
        killed = text.trim() == KILLED;
        let code = text.trim().parse::<i32>().ok();
        (
            if killed {
                "killed".to_string()
            } else {
                format!(
                    "exit {}",
                    code.map_or_else(|| "?".to_string(), |code| code.to_string())
                )
            },
            code,
            false,
        )
    } else {
        ("dead".to_string(), None, false)
    };
    let preview = if log.is_file() {
        preview(&log, 8 * 1024)?
    } else {
        String::new()
    };
    Ok(Status {
        text: format!(
            "job {id}\ncommand: {command}\nworkspace: {cwd}\npid: {pid}\nstate: {state}\nlog: {log} ({bytes} bytes)\n{preview}",
            id = record.id,
            command = record.command,
            cwd = record.workspace,
            pid = record.pid,
            log = log.display(),
        ),
        exit_code,
        running,
        killed,
    })
}

pub(crate) fn preview(path: &Path, cap: usize) -> Result<String> {
    let meta = std::fs::metadata(path)?;
    let len = meta.len();
    let mut file = std::fs::File::open(path)?;
    if len == 0 {
        return Ok(String::new());
    }
    if len <= cap as u64 {
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        return Ok(String::from_utf8_lossy(&buf).into_owned());
    }
    let head_len = cap * 3 / 4;
    let tail_len = cap - head_len;
    let mut head = vec![0_u8; head_len];
    file.read_exact(&mut head)?;
    file.seek(SeekFrom::Start(len.saturating_sub(tail_len as u64)))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    Ok(format!(
        "{}\n\n[... {len} bytes in {} ...]\n\n{}",
        String::from_utf8_lossy(&head),
        path.display(),
        String::from_utf8_lossy(&tail)
    ))
}

pub(crate) fn stamp(
    id: &str,
    pid: u32,
    command: &str,
    workspace: &Path,
    background: bool,
    session: Option<String>,
) -> Result<Record> {
    let started = crate::mail::now();
    Ok(Record {
        id: id.to_string(),
        pid,
        command: command.to_string(),
        workspace: workspace.display().to_string(),
        started,
        background,
        sandbox: false,
        session,
        log: None,
    })
}

pub(crate) fn write_killed(path: &Path) -> Result<()> {
    store::write(path, format!("{KILLED}\n"))?;
    Ok(())
}

pub(crate) fn exit_path(workspace: &Path, id: &str) -> PathBuf {
    dir(workspace).join(format!("{id}.exit"))
}

pub(crate) fn log_path(workspace: &Path, record: &Record) -> PathBuf {
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
    let mut pause = POLL_START_SECS;
    let mut announced = false;
    loop {
        let current = status(workspace, &load(workspace, id)?)?;
        let elapsed = started.elapsed().as_secs();
        if !current.running || elapsed >= limit_secs {
            return Ok(current);
        }
        if !announced {
            announced = true;
            waiting()?;
        }
        tokio::time::sleep(crate::limits::seconds(pause.min(limit_secs - elapsed))).await;
        pause = (pause * 2).min(POLL_CAP_SECS);
    }
}

pub(crate) fn alive_since(pid: u32, started: u64) -> bool {
    #[cfg(windows)]
    {
        windows::started(pid).is_some_and(|created| created <= started.saturating_add(5))
    }
    #[cfg(unix)]
    {
        alive(pid) && start_secs(pid).is_none_or(|created| created <= started.saturating_add(5))
    }
}

pub(crate) fn write_exit(path: &Path, code: i32) -> Result<()> {
    store::write(path, format!("{code}\n"))?;
    Ok(())
}

#[cfg(unix)]
#[expect(unsafe_code, reason = "std has no signal-zero liveness probe")]
pub(crate) fn alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: kill with signal 0 reads two integers, sends nothing, and touches no memory.
    let sent = unsafe { libc::kill(pid, 0) } == 0;
    sent || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "std has no process start-time query")]
fn start_secs(pid: u32) -> Option<u64> {
    let pid = libc::c_int::try_from(pid).ok()?;
    // SAFETY: proc_bsdinfo is plain integers and byte arrays; all-zero is a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: the buffer pointer and size describe one owned proc_bsdinfo for the call.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            std::ptr::from_mut(&mut info).cast(),
            size,
        )
    };
    (written == size).then_some(info.pbi_start_tvsec)
}

#[cfg(target_os = "linux")]
#[expect(unsafe_code, reason = "std has no clock-tick query")]
fn start_secs(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let ticks = stat
        .rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse::<u64>()
        .ok()?;
    let boot = std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse::<u64>()
        .ok()?;
    // SAFETY: sysconf reads one integer and touches no memory.
    let hertz = u64::try_from(unsafe { libc::sysconf(libc::_SC_CLK_TCK) }).ok()?;
    (hertz > 0).then(|| boot + ticks / hertz)
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
fn start_secs(_pid: u32) -> Option<u64> {
    None
}

#[cfg(windows)]
#[path = "jobs/windows.rs"]
mod windows;

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
