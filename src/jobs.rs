//! Durable command logs under `<workspace>/.mage/jobs`.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const ID_MAX: usize = 80;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub id: String,
    pub pid: u32,
    pub command: String,
    pub workspace: String,
    pub started: u64,
    pub background: bool,
}

pub(crate) struct Status {
    pub text: String,
    pub exit_code: Option<i32>,
    pub running: bool,
}

pub(crate) fn dir(workspace: &Path) -> PathBuf {
    workspace.join(".mage").join("jobs")
}

pub(crate) fn prepare(workspace: &Path) -> Result<(String, PathBuf, PathBuf, PathBuf)> {
    let root = dir(workspace);
    std::fs::create_dir_all(&root)?;
    let ignore = root.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(ignore, "*\n")?;
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
    let text = serde_json::to_string(record)?;
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)?;
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

pub(crate) fn status(workspace: &Path, id: &str) -> Result<Status> {
    let record = load(workspace, id)?;
    let root = dir(workspace);
    let log = root.join(format!("{}.log", record.id));
    let exit = root.join(format!("{}.exit", record.id));
    let bytes = std::fs::metadata(&log).map(|meta| meta.len()).unwrap_or(0);
    let (state, exit_code, running) = if exit.is_file() {
        let code = std::fs::read_to_string(&exit)
            .ok()
            .and_then(|code| code.trim().parse::<i32>().ok());
        (
            format!(
                "exit {}",
                code.map_or_else(|| "?".to_string(), |code| code.to_string())
            ),
            code,
            false,
        )
    } else if alive(record.pid) {
        ("running".to_string(), None, true)
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

pub(crate) fn wrap(command: &str, exit: &Path) -> String {
    format!(
        "trap '' HUP\n(\n{command}\n)\ncode=$?\nprintf '%s\\n' \"$code\" > '{}'\nexit \"$code\"",
        exit.display()
    )
}

pub(crate) fn stamp(
    id: &str,
    pid: u32,
    command: &str,
    workspace: &Path,
    background: bool,
) -> Result<Record> {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Config(error.to_string()))?
        .as_secs();
    Ok(Record {
        id: id.to_string(),
        pid,
        command: command.to_string(),
        workspace: workspace.display().to_string(),
        started,
        background,
    })
}

pub(crate) fn write_exit(path: &Path, code: i32) -> Result<()> {
    let mut file = std::fs::File::create(path)?;
    writeln!(file, "{code}")?;
    Ok(())
}

pub(crate) fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

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
