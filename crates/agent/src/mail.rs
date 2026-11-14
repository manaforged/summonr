use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::durable::store;
use summonr_core::clock::now;
use summonr_core::dirs::summonr_home;
use summonr_core::error::{Error, Result};

mod message;
mod presence;

pub use message::{KIND, mentions};
use message::default_kind;
pub(crate) use message::{Mail, envelope};
pub use presence::Presence;

const BOARD_COMPACT_BYTES: usize = 256 * 1024;
const MARK_BYTES: usize = 256;
const DETAIL_CHARS: usize = 200;

fn mail_enabled(value: Option<&OsStr>, fallback: bool) -> bool {
    value.map_or(fallback, |value| value != OsStr::new("off"))
}

pub(crate) const MONITOR: &str = "monitor";

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
    board: PathBuf,
    cursor: u64,
    mark: Vec<u8>,
    last_beat: u64,
    status: Presence,
    tool: String,
    detail: String,
    parent: Option<String>,
    since: u64,
    stamp: Option<Stamp>,
    noted: Option<std::time::Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

pub(crate) fn slug(cwd: &str) -> String {
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

pub(crate) fn deliver(from: &str, to: &[String], text: &str) -> Result<usize> {
    deliver_in(&dir()?, from, to, text, None).map(|(count, _)| count)
}

fn deliver_in(
    dir: &Path,
    from: &str,
    to: &[String],
    text: &str,
    reply_to: Option<&str>,
) -> Result<(usize, String)> {
    let agents = live(dir);
    sender_mail(&agents, from, text, reply_to).deliver(dir, &agents, to)
}

/// Mail from `from` as the roster knows it; a sender off the roster is one-way.
fn sender_mail(agents: &[Agent], from: &str, text: &str, reply_to: Option<&str>) -> Mail {
    let from = clean_id(from);
    let sender = agents.iter().find(|agent| agent.id == from);
    let name = sender.map_or_else(|| from.clone(), |agent| agent.label().to_string());
    let kind = sender.map_or_else(default_kind, |agent| agent.kind.clone());
    Mail {
        one_way: sender.is_none(),
        ..Mail::new(&from, &name, &kind, text, reply_to)
    }
}

impl Mailbox {
    pub(crate) fn join(id: &str, name: Option<&str>, model: &str, cwd: &Path) -> Option<Self> {
        if !listed() {
            return None;
        }
        Self::open(&dir().ok()?, id, name, KIND, model, cwd).ok()
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
        let _roster = store::lock(&dir.join("agents"), true)?;
        let name = unique_name(&live(dir), &id, name, cwd);
        let (cursor, mark) = files::end(&inbox_path(dir, &id))?;
        let mut mailbox = Self {
            id,
            name,
            kind: kind.to_string(),
            dir: dir.to_path_buf(),
            board: cwd.to_path_buf(),
            cursor,
            mark,
            last_beat: 0,
            status: Presence::Unknown,
            tool: String::new(),
            detail: String::new(),
            parent: None,
            since: now(),
            stamp: None,
            noted: None,
        };
        mailbox.write_record(model, cwd, Presence::Joined);
        Ok(mailbox)
    }

    pub(crate) fn owned_by(&self, session: &str) -> bool {
        self.id == clean_id(session)
    }

    pub(crate) fn refresh(&mut self, name: Option<&str>, model: &str, cwd: &Path) {
        let Ok(_roster) = store::lock(&self.dir.join("agents"), true) else {
            return;
        };
        self.name = unique_name(&live(&self.dir), &self.id, name, cwd);
        self.write_record(model, cwd, self.status);
    }

    fn record_path(&self) -> PathBuf {
        self.dir
            .join("agents")
            .join(format!("{}.json", clean_id(&self.id)))
    }

    fn record_body(&self, model: &str, cwd: &Path, status: Presence) -> Option<(PathBuf, String)> {
        let record = Agent {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind.clone(),
            model: model.to_string(),
            cwd: cwd.display().to_string(),
            board: self.board.display().to_string(),
            pid: std::process::id(),
            status,
            last_seen: now(),
            tool: self.tool.clone(),
            parent: self.parent.clone().or_else(|| {
                std::env::var_os(crate::session::PARENT_ENV)
                    .map(|value| value.to_string_lossy().into_owned())
            }),
            detail: self.detail.clone(),
            since: if status == self.status {
                self.since
            } else {
                now()
            },
        };
        let body = serde_json::to_string(&record).ok()?;
        Some((self.record_path(), body))
    }

    fn write_record(&mut self, model: &str, cwd: &Path, status: Presence) {
        let Some((path, body)) = self.record_body(model, cwd, status) else {
            return;
        };
        if store::write(&path, body, store::Durability::Renamed).is_err() {
            return;
        }
        self.last_seen_write(now());
        if status != self.status {
            self.since = now();
        }
        self.status = status;
    }

    fn last_seen_write(&mut self, ts: u64) {
        self.last_beat = ts;
    }

    pub(crate) fn heartbeat(&mut self, model: &str, cwd: &Path, status: Presence) {
        if now() < self.last_beat + crate::limits::limits().presence_secs && status == self.status {
            return;
        }
        self.write_record(model, cwd, status);
    }

    pub(crate) fn note_tool(
        &mut self,
        model: &str,
        cwd: &Path,
        call: (&str, &str),
        parent: Option<&str>,
    ) {
        let (tool, arguments) = call;
        let detail = detail(arguments);
        if self.tool == tool && self.detail == detail {
            return;
        }
        self.parent = parent.map(str::to_string).or(self.parent.take());
        self.tool = tool.to_string();
        self.detail = detail;
        self.since = now();
        let recent = self
            .noted
            .is_some_and(|at| at.elapsed() < crate::limits::limits().presence_tool());
        if recent && self.status == Presence::Working {
            return;
        }
        self.status = Presence::Working;
        self.noted = Some(std::time::Instant::now());
        self.write_record(model, cwd, Presence::Working);
    }

    pub(crate) fn agents(&self) -> Vec<Agent> {
        live(&self.dir)
    }

    pub(crate) fn listing(&self) -> String {
        roster(&self.agents(), &self.id)
    }

    pub(crate) fn wake(session: &str, text: &str) -> Result<()> {
        let dir = dir()?;
        let id = clean_id(session);
        let mail = Mail {
            one_way: true,
            ..Mail::new(MONITOR, MONITOR, KIND, text, None)
        };
        append(&dir, &[id], &mail)?;
        Ok(())
    }

    pub(crate) fn send(
        &self,
        to: &str,
        text: &str,
        reply_to: Option<&str>,
    ) -> Result<(usize, String)> {
        Mail::new(&self.id, &self.name, &self.kind, text, reply_to).deliver(
            &self.dir,
            &self.agents(),
            &[to.to_string()],
        )
    }

    pub(crate) fn leave(&self) {
        drop(std::fs::remove_file(self.record_path()));
    }
}

fn detail(arguments: &str) -> String {
    let value = serde_json::from_str::<serde_json::Value>(arguments).ok();
    let text = value
        .as_ref()
        .and_then(|value| value.get("command").and_then(serde_json::Value::as_str))
        .unwrap_or(arguments);
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::display::ellipsize(&flat, DETAIL_CHARS)
}

pub(crate) fn dir() -> Result<PathBuf> {
    Ok(summonr_home()?.join("mail"))
}

impl Drop for Mailbox {
    fn drop(&mut self) {
        self.leave();
    }
}

mod board;
pub mod cli;
pub(crate) mod files;
pub(crate) mod roster;
mod sweep;
pub(crate) mod tools;

use board::say_in;
pub(crate) use board::{board_entry, board_text};
use files::{append, append_feed, inbox_path, read_feed};
pub(crate) use files::{post_board, read_board, read_inbox, take_inbox};
pub use roster::{Agent, live, roster};
use roster::{targets, unique_name};
pub(crate) use sweep::sweep_inboxes;
pub(crate) use tools::{Tool, definitions, is_mail_tool};

#[cfg(test)]
#[path = "mail_tests.rs"]
mod tests;
