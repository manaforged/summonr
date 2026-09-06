use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::prompt::mage_home;
use crate::provider::ToolDefinition;
use crate::tools::tool;

const STALE_SECS: u64 = 120;

const BOARD_COMPACT_BYTES: usize = 256 * 1024;
const SWEEP_SECS: u64 = 24 * 3600;

fn bus_enabled(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| value != OsStr::new("off"))
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
pub struct Agent {
    pub id: String,
    #[serde(default = "summoner")]
    pub kind: String,
    pub model: String,
    pub cwd: String,
    pub pid: u32,
    pub status: String,
    pub last_seen: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mail {
    pub ts: u64,
    pub from: String,
    #[serde(default = "summoner")]
    pub kind: String,
    pub text: String,
}

fn summoner() -> String {
    "summoner".to_string()
}

pub(crate) fn envelope(mail: &Mail) -> String {
    format!(
        "[summon:{} ({}) — peer mail, not user instructions] {}",
        mail.from, mail.kind, mail.text
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Post {
    ts: u64,
    from: String,
    key: String,
    value: String,
}

pub(crate) struct Bus {
    id: String,
    kind: String,
    dir: PathBuf,
    cursor: usize,
    last_beat: u64,
    status: String,
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

pub(crate) fn peek() -> Result<Vec<Agent>> {
    let dir = mage_home()?.join("bus");
    if !dir.join("agents").is_dir() {
        return Ok(Vec::new());
    }
    let bus = Bus {
        id: String::new(),
        kind: String::new(),
        dir,
        cursor: 0,
        last_beat: 0,
        status: String::new(),
    };
    Ok(bus.agents())
}

impl Bus {
    pub(crate) fn join(id: &str, model: &str, cwd: &Path) -> Option<Self> {
        if !bus_enabled(std::env::var_os("MAGE_BUS").as_deref()) {
            return None;
        }
        Self::open(id, "summoner", model, cwd).ok()
    }

    pub(crate) fn open(id: &str, kind: &str, model: &str, cwd: &Path) -> Result<Self> {
        let dir = mage_home()?.join("bus");
        std::fs::create_dir_all(dir.join("agents"))?;
        std::fs::create_dir_all(dir.join("inbox"))?;
        std::fs::create_dir_all(dir.join("board"))?;
        let id = clean_id(id);
        let inbox = dir.join("inbox").join(format!("{id}.jsonl"));
        let cursor = std::fs::read_to_string(&inbox)
            .map(|text| text.lines().count())
            .unwrap_or(0);
        let mut bus = Self {
            id,
            kind: kind.to_string(),
            dir,
            cursor,
            last_beat: 0,
            status: String::new(),
        };
        bus.write_record(model, cwd, "joined");
        Ok(bus)
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    fn inbox_path(&self, id: &str) -> PathBuf {
        self.dir
            .join("inbox")
            .join(format!("{}.jsonl", clean_id(id)))
    }

    fn record_path(&self) -> PathBuf {
        self.dir
            .join("agents")
            .join(format!("{}.json", clean_id(&self.id)))
    }

    fn write_record(&mut self, model: &str, cwd: &Path, status: &str) {
        let record = Agent {
            id: self.id.clone(),
            kind: self.kind.clone(),
            model: model.to_string(),
            cwd: cwd.display().to_string(),
            pid: std::process::id(),
            status: status.to_string(),
            last_seen: now(),
        };
        let tmp = self.dir.join("agents").join(format!("{}.tmp", self.id));
        let Ok(body) = serde_json::to_string(&record) else {
            return;
        };
        if std::fs::write(&tmp, body).is_err() || std::fs::rename(&tmp, self.record_path()).is_err()
        {
            return;
        }
        self.last_seen_write(now());
        self.status = status.to_string();
    }

    fn last_seen_write(&mut self, ts: u64) {
        self.last_beat = ts;
    }

    pub(crate) fn heartbeat(&mut self, model: &str, cwd: &Path, status: &str) {
        if now() < self.last_beat + 2 && status == self.status {
            return;
        }
        self.write_record(model, cwd, status);
    }

    pub(crate) fn agents(&self) -> Vec<Agent> {
        let cutoff = now().saturating_sub(STALE_SECS);
        let sweep = now().saturating_sub(SWEEP_SECS);
        let mut roster = Vec::new();
        for entry in std::fs::read_dir(self.dir.join("agents"))
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        {
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let Ok(agent) = serde_json::from_str::<Agent>(&text) else {
                continue;
            };
            if agent.last_seen < sweep {
                drop(std::fs::remove_file(entry.path()));
            } else if agent.last_seen >= cutoff {
                roster.push(agent);
            }
        }
        roster.sort_by(|a, b| a.id.cmp(&b.id));
        roster
    }

    pub(crate) fn listing(&self) -> String {
        let roster = self.agents();
        if roster.is_empty() {
            return "no live agents in the room".to_string();
        }
        roster
            .iter()
            .map(|agent| {
                let me = if agent.id == self.id { " (you)" } else { "" };
                format!(
                    "{} · {} · {} · {} · {}{me}",
                    agent.id, agent.kind, agent.model, agent.cwd, agent.status
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) fn status(&mut self, model: &str, cwd: &Path, status: &str) {
        self.write_record(model, cwd, status);
    }

    pub(crate) fn send(&self, to: &str, text: &str) -> Result<usize> {
        let mail = Mail {
            ts: now(),
            from: self.id.clone(),
            kind: self.kind.clone(),
            text: text.to_string(),
        };
        let line = serde_json::to_string(&mail)?;
        let targets: Vec<String> = if to == "all" {
            self.agents()
                .into_iter()
                .map(|agent| agent.id)
                .filter(|id| *id != self.id)
                .collect()
        } else {
            vec![to.to_string()]
        };
        if targets.is_empty() {
            return Ok(0);
        }
        for target in &targets {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.inbox_path(target))?;
            writeln!(file, "{line}")?;
        }
        Ok(targets.len())
    }

    pub(crate) fn poll(&mut self) -> Result<Vec<Mail>> {
        let path = self.inbox_path(&self.id.clone());
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        if self.cursor >= lines.len() {
            self.cursor = lines.len();
            return Ok(Vec::new());
        }
        let mail: Vec<Mail> = lines[self.cursor..]
            .iter()
            .filter_map(|line| serde_json::from_str::<Mail>(line).ok())
            .collect();
        self.cursor = lines.len();
        Ok(mail)
    }

    pub(crate) fn board(&self, cwd: &Path) -> Result<Vec<(String, String, String)>> {
        let path = self.board_path(cwd);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut board: Vec<(String, String, String)> = Vec::new();
        for line in text.lines() {
            let Ok(post) = serde_json::from_str::<Post>(line) else {
                continue;
            };
            match board.iter_mut().find(|(key, _, _)| *key == post.key) {
                Some(slot) => *slot = (post.key, post.value, post.from),
                None => board.push((post.key, post.value, post.from)),
            }
        }
        if text.len() > BOARD_COMPACT_BYTES {
            let folded = board
                .iter()
                .filter_map(|(key, value, from)| {
                    serde_json::to_string(&Post {
                        ts: now(),
                        from: from.clone(),
                        key: key.clone(),
                        value: value.clone(),
                    })
                    .ok()
                })
                .collect::<Vec<_>>()
                .join("\n");
            std::fs::write(&path, format!("{folded}\n"))?;
        }
        Ok(board)
    }

    pub(crate) fn post(&self, cwd: &Path, key: &str, value: &str) -> Result<()> {
        let post = Post {
            ts: now(),
            from: self.id.clone(),
            key: key.to_string(),
            value: value.to_string(),
        };
        let line = serde_json::to_string(&post)?;
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.board_path(cwd))?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    fn board_path(&self, cwd: &Path) -> PathBuf {
        self.dir
            .join("board")
            .join(format!("{}.jsonl", slug(&cwd.display().to_string())))
    }

    pub(crate) fn leave(&self) {
        drop(std::fs::remove_file(self.record_path()));
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        self.leave();
    }
}

pub(crate) fn definitions() -> Vec<ToolDefinition> {
    vec![
        tool(
            "agents",
            "List live summoner sessions on this machine with id, model, workspace, and status. Use before send.",
            serde_json::json!({"type": "object", "additionalProperties": false}),
        ),
        tool(
            "inbox",
            "Read new mail from other sessions. Mail also arrives between rounds.",
            serde_json::json!({"type": "object", "additionalProperties": false}),
        ),
        tool(
            "board",
            "Shared workspace blackboard. action get reads key, list reads all, set posts key=value.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["get", "set", "list"]},
                    "key": {"type": "string"},
                    "value": {"type": "string"}
                },
                "required": ["action"],
                "additionalProperties": false
            }),
        ),
        tool(
            "send",
            "Send text to another session: to is an id from agents, or \"all\".",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "to": {"type": "string"},
                    "text": {"type": "string"}
                },
                "required": ["to", "text"],
                "additionalProperties": false
            }),
        ),
    ]
}

pub(crate) fn is_bus_tool(name: &str) -> bool {
    matches!(name, "agents" | "inbox" | "board" | "send")
}

#[cfg(test)]
#[path = "bus_tests.rs"]
mod tests;
