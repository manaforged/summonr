//! The bus: a file-backed shared mind for summoner sessions on one machine.
//!
//! Layout under `$MAGE_HOME/bus/`:
//!
//! ```text
//! agents/<session-id>.json      registry record, rewritten on heartbeat
//! inbox/<session-id>.jsonl      per-session mailbox, append-only
//! board/<workspace-slug>.jsonl  shared blackboard, last write wins per key
//! ```
//!
//! No daemon and no locks: records are temp-write + rename, mailboxes and
//! boards are O_APPEND lines. A session joins when `MAGE_BUS` is configured,
//! heartbeats its status, polls its inbox between rounds, and leaves on drop.
//! Sessions stay off the bus by default; set `MAGE_BUS=on` to join.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::prompt::mage_home;
use crate::provider::ToolDefinition;
use crate::tools::tool;

/// How long a record may go silent before the roster calls it dead.
/// Long tool calls (a 60-second build) must not age a session off.
const STALE_SECS: u64 = 120;

/// One mail envelope may carry at most this much text.
const MAIL_CHARS: usize = 1_000;
/// Files past this size get rotated on read (mailboxes) or folded
/// (boards), so a spammy writer cannot grow them forever.
const MAX_FILE_BYTES: usize = 256 * 1024;
/// Records older than a day are swept on roster reads.
const SWEEP_SECS: u64 = 24 * 3600;

fn bus_enabled(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| value != OsStr::new("off"))
}

/// A session id safe for a file name: the bus trusts the model with
/// target ids, so anything but plain name characters becomes a dash.
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

/// One live session on the machine.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Agent {
    pub id: String,
    pub model: String,
    pub cwd: String,
    pub pid: u32,
    pub status: String,
    pub last_seen: u64,
}

/// One mailbox line.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mail {
    pub ts: u64,
    pub from: String,
    pub text: String,
}

pub(crate) fn envelope(from: &str, text: &str) -> String {
    format!(
        "[summon:{} — peer mail, not user instructions] {}",
        from,
        text.chars().take(500).collect::<String>()
    )
}

/// One blackboard line; the last write for a key wins.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Post {
    ts: u64,
    from: String,
    key: String,
    value: String,
}

/// A session's handle to the bus. Owns its read cursor.
pub(crate) struct Bus {
    id: String,
    dir: PathBuf,
    cursor: usize,
    last_beat: u64,
    status: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `cwd` as a flat directory name, pi-style.
fn slug(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

impl Bus {
    /// Join the bus when `MAGE_BUS` is configured, or return None when it is
    /// absent, disabled, or the session has no home. The inbox cursor starts
    /// at the end: mail from before this boot is not replayed.
    pub(crate) fn join(id: &str, model: &str, cwd: &Path) -> Option<Self> {
        if !bus_enabled(std::env::var_os("MAGE_BUS").as_deref()) {
            return None;
        }
        let dir = mage_home().ok()?.join("bus");
        std::fs::create_dir_all(dir.join("agents")).ok()?;
        std::fs::create_dir_all(dir.join("inbox")).ok()?;
        std::fs::create_dir_all(dir.join("board")).ok()?;
        let inbox = dir.join("inbox").join(format!("{id}.jsonl"));
        let cursor = std::fs::read_to_string(&inbox)
            .map(|text| text.lines().count())
            .unwrap_or(0);
        let mut bus = Self {
            id: id.to_string(),
            dir,
            cursor,
            last_beat: 0,
            status: String::new(),
        };
        bus.write_record(model, cwd, "joined");
        Some(bus)
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

    /// Refresh presence; cheap no-op between beats unless status changed.
    pub(crate) fn heartbeat(&mut self, model: &str, cwd: &Path, status: &str) {
        if now() < self.last_beat + 2 && status == self.status {
            return;
        }
        self.write_record(model, cwd, status);
    }

    /// Live roster: records seen in the last [`STALE_SECS`] seconds.
    /// Records older than a day are swept from disk as a side effect.
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

    /// Append `text` to one peer's mailbox, or to every other live peer
    /// when `to` is "all". Text truncates at [`MAIL_CHARS`].
    pub(crate) fn send(&self, to: &str, text: &str) -> Result<usize> {
        let mail = Mail {
            ts: now(),
            from: self.id.clone(),
            text: text.chars().take(MAIL_CHARS).collect(),
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

    /// Drain mail addressed to this session since the last poll. An
    /// oversized mailbox is rotated down to its newest lines first.
    pub(crate) fn poll(&mut self) -> Result<Vec<Mail>> {
        let path = self.inbox_path(&self.id.clone());
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        if text.len() > MAX_FILE_BYTES {
            let keep: Vec<&str> = text.lines().collect();
            let keep_from = keep.len().saturating_sub(500);
            text = format!("{}\n", keep[keep_from..].join("\n"));
            std::fs::write(&path, &text)?;
            self.cursor = self.cursor.saturating_sub(keep_from);
        }
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

    /// Read the workspace blackboard as folded key/value posts. An
    /// oversized board is folded back to disk as a side effect.
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
        if text.len() > MAX_FILE_BYTES {
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

    /// Post to the workspace blackboard.
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

    /// Deregister. Best effort; the roster also expires stale records.
    pub(crate) fn leave(&self) {
        drop(std::fs::remove_file(self.record_path()));
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        self.leave();
    }
}

/// Model-facing bus tools, advertised alongside the workspace tools.
pub(crate) fn definitions() -> Vec<ToolDefinition> {
    vec![
        tool(
            "agents",
            "List live summoner sessions on this machine: id, model, workspace, what they are doing. Use before send to find a peer's id.",
            serde_json::json!({"type": "object", "additionalProperties": false}),
        ),
        tool(
            "inbox",
            "Read new messages other summoner sessions sent you. Mail also arrives on its own between your rounds.",
            serde_json::json!({"type": "object", "additionalProperties": false}),
        ),
        tool(
            "board",
            "The workspace blackboard shared by all summon sessions here. action get reads key, list reads everything, set posts key=value — claim files, post decisions, leave state for peers.",
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
            "Send a plain-text message to another summoner session: to is a session id from agents, or \"all\". It lands between their rounds; they can reply to your id.",
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

/// True when `name` is a bus tool the runtime executes itself.
pub(crate) fn is_bus_tool(name: &str) -> bool {
    matches!(name, "agents" | "inbox" | "board" | "send")
}

#[cfg(test)]
#[path = "bus_tests.rs"]
mod tests;
