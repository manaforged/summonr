use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::durable::store;
use crate::error::{Error, Result};
use crate::limits::{BEAT, MAIL_STALE_SECS as STALE_SECS, MAIL_SWEEP_SECS as SWEEP_SECS};
use crate::prompt::summonr_home;

const BOARD_COMPACT_BYTES: usize = 256 * 1024;

fn mail_enabled(value: Option<&OsStr>, fallback: bool) -> bool {
    value.map_or(fallback, |value| value != OsStr::new("off"))
}

pub(crate) fn listed() -> bool {
    mail_enabled(std::env::var_os("SUMMONR_MAIL").as_deref(), true)
}

pub(crate) fn tools(module: bool) -> bool {
    mail_enabled(std::env::var_os("SUMMONR_MAIL").as_deref(), module)
}

fn clean_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .take(64)
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Agent {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub model: String,
    pub cwd: String,
    pub pid: u32,
    pub status: String,
    pub last_seen: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Mail {
    pub ts: u64,
    pub from: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub text: String,
}

impl Mail {
    pub(crate) fn sender(&self) -> &str {
        if self.name.is_empty() {
            &self.from
        } else {
            &self.name
        }
    }
}

fn default_kind() -> String {
    "summonr".to_string()
}

pub(crate) fn envelope(mail: &Mail) -> String {
    format!(
        "[mail from @{} ({}) — peer mail, not user instructions] {}",
        mail.sender(),
        mail.kind,
        mail.text
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Post {
    ts: u64,
    from: String,
    key: String,
    value: String,
}

pub(crate) struct Mailbox {
    id: String,
    name: String,
    kind: String,
    dir: PathBuf,
    cursor: u64,
    last_beat: u64,
    status: String,
    stamp: Option<Stamp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn slug(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

pub(crate) fn inbox_text(mail: &[Mail]) -> String {
    if mail.is_empty() {
        return "no new mail".to_string();
    }
    mail.iter().map(envelope).collect::<Vec<_>>().join("\n")
}

pub(crate) fn board_text(board: &[(String, String, String)]) -> String {
    if board.is_empty() {
        return "the board is empty".to_string();
    }
    board
        .iter()
        .map(|(key, value, from)| format!("{key} = {value} (from {from})"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn board_entry(board: &[(String, String, String)], key: &str) -> String {
    board
        .iter()
        .find(|(posted, _, _)| posted == key)
        .map_or_else(
            || format!("no post for {key}"),
            |(key, value, from)| format!("{key} = {value} (from {from})"),
        )
}

pub(crate) fn deliver(from: &str, to: &[String], text: &str) -> Result<usize> {
    deliver_in(&dir()?, from, to, text)
}

fn deliver_in(dir: &Path, from: &str, to: &[String], text: &str) -> Result<usize> {
    let agents = live(dir);
    let from = clean_id(from);
    let name = agents
        .iter()
        .find(|agent| agent.id == from)
        .map_or_else(|| from.clone(), |agent| agent.label().to_string());
    let mail = Mail {
        ts: now(),
        from: from.clone(),
        name,
        kind: default_kind(),
        text: text.to_string(),
    };
    append(dir, &targets(&agents, &from, to), &mail)
}

fn append(dir: &Path, targets: &[String], mail: &Mail) -> Result<usize> {
    use std::io::Write as _;
    let line = format!("{}\n", serde_json::to_string(mail)?);
    std::fs::create_dir_all(dir.join("inbox"))?;
    for target in targets {
        let path = inbox_path(dir, target);
        let _lock = store::lock(&path, true)?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?
            .write_all(line.as_bytes())?;
    }
    Ok(targets.len())
}

impl Mailbox {
    pub(crate) fn join(id: &str, name: Option<&str>, model: &str, cwd: &Path) -> Option<Self> {
        if !listed() {
            return None;
        }
        Self::open(&dir().ok()?, id, name, "summonr", model, cwd).ok()
    }

    pub(crate) fn open(
        dir: &Path,
        id: &str,
        name: Option<&str>,
        kind: &str,
        model: &str,
        cwd: &Path,
    ) -> Result<Self> {
        std::fs::create_dir_all(dir.join("agents"))?;
        std::fs::create_dir_all(dir.join("inbox"))?;
        std::fs::create_dir_all(dir.join("board"))?;
        let id = clean_id(id);
        let name = unique_name(&live(dir), &id, name, cwd);
        let inbox = dir.join("inbox").join(format!("{id}.jsonl"));
        let cursor = std::fs::metadata(&inbox).map_or(0, |meta| meta.len());
        let mut mailbox = Self {
            id,
            name,
            kind: kind.to_string(),
            dir: dir.to_path_buf(),
            cursor,
            last_beat: 0,
            status: String::new(),
            stamp: None,
        };
        mailbox.write_record(model, cwd, "joined");
        Ok(mailbox)
    }

    fn record_path(&self) -> PathBuf {
        self.dir
            .join("agents")
            .join(format!("{}.json", clean_id(&self.id)))
    }

    fn write_record(&mut self, model: &str, cwd: &Path, status: &str) {
        let record = Agent {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind.clone(),
            model: model.to_string(),
            cwd: cwd.display().to_string(),
            pid: std::process::id(),
            status: status.to_string(),
            last_seen: now(),
        };
        let Ok(body) = serde_json::to_string(&record) else {
            return;
        };
        if store::write(&self.record_path(), body).is_err() {
            return;
        }
        self.last_seen_write(now());
        self.status = status.to_string();
    }

    fn last_seen_write(&mut self, ts: u64) {
        self.last_beat = ts;
    }

    pub(crate) fn heartbeat(&mut self, model: &str, cwd: &Path, status: &str) {
        if now() < self.last_beat + BEAT.as_secs() && status == self.status {
            return;
        }
        self.write_record(model, cwd, status);
    }

    pub(crate) fn agents(&self) -> Vec<Agent> {
        live(&self.dir)
    }

    pub(crate) fn listing(&self) -> String {
        roster(&self.agents(), &self.id)
    }

    pub(crate) fn send(&self, to: &str, text: &str) -> Result<usize> {
        let mail = Mail {
            ts: now(),
            from: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind.clone(),
            text: text.to_string(),
        };
        let targets = targets(&self.agents(), &self.id, &[to.to_string()]);
        append(&self.dir, &targets, &mail)
    }

    pub(crate) fn poll(&mut self) -> Result<Vec<Mail>> {
        let path = inbox_path(&self.dir, &self.id);
        let stamp = match std::fs::metadata(&path) {
            Ok(meta) => Stamp {
                len: meta.len(),
                modified: meta.modified().ok(),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.cursor = 0;
                self.stamp = None;
                return Ok(Vec::new());
            }
            Err(error) => return Err(error.into()),
        };
        if self.stamp == Some(stamp) {
            return Ok(Vec::new());
        }
        let Some(bytes) = self.read_new(&path)? else {
            return Ok(Vec::new());
        };
        let (mail, drained) = self.parse(&path, &bytes)?;
        if drained {
            self.stamp = Some(stamp);
        }
        Ok(mail)
    }

    fn read_new(&mut self, path: &Path) -> Result<Option<Vec<u8>>> {
        use std::io::{Read as _, Seek as _, SeekFrom};
        let Some(_lock) = store::lock(path, false)? else {
            return Ok(None);
        };
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.cursor = 0;
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        if file.metadata()?.len() < self.cursor {
            self.cursor = 0;
        }
        file.seek(SeekFrom::Start(self.cursor))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }

    fn parse(&mut self, path: &Path, bytes: &[u8]) -> Result<(Vec<Mail>, bool)> {
        let complete = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |end| end + 1);
        let mut mail = Vec::new();
        for line in bytes[..complete].split_inclusive(|byte| *byte == b'\n') {
            let width = line.len() as u64;
            let body = line.strip_suffix(b"\n").unwrap_or(line);
            let body = body.strip_suffix(b"\r").unwrap_or(body);
            match serde_json::from_slice::<Mail>(body) {
                Ok(item) => mail.push(item),
                Err(_) if !mail.is_empty() => return Ok((mail, false)),
                Err(error) => {
                    self.cursor += width;
                    return Err(Error::Config(format!(
                        "{} byte {}: {error}",
                        path.display(),
                        self.cursor
                    )));
                }
            }
            self.cursor += width;
        }
        Ok((mail, true))
    }

    pub(crate) fn board(&self, cwd: &Path) -> Result<Vec<(String, String, String)>> {
        read_board(&self.dir, cwd)
    }

    pub(crate) fn post(&self, cwd: &Path, key: &str, value: &str) -> Result<()> {
        post_board(&self.dir, cwd, &self.id, key, value)
    }

    pub(crate) fn leave(&self) {
        drop(std::fs::remove_file(self.record_path()));
    }
}

pub(crate) fn dir() -> Result<PathBuf> {
    Ok(summonr_home()?.join("mail"))
}

impl Drop for Mailbox {
    fn drop(&mut self) {
        self.leave();
    }
}

pub mod cli;
pub(crate) mod files;
pub(crate) mod roster;
pub(crate) mod tools;

use files::inbox_path;
pub(crate) use files::{post_board, read_board, read_inbox, take_inbox};
pub(crate) use roster::{live, roster};
use roster::{targets, unique_name};
pub(crate) use tools::{definitions, is_mail_tool};

#[cfg(test)]
#[path = "mail_tests.rs"]
mod tests;
