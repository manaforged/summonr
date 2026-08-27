//! `session.jsonl`: the conversation, one JSON line per event, append-only.
//!
//! Resume reads the file front to back and folds it into [`State`]. A line
//! the dying process did not finish writing is dropped on read.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::id::SessionId;
use crate::provider::{Image, Message, Role, ToolCall};
use crate::session::Replay;

const FILE: &str = "session.jsonl";
const WORK: &str = "work.json";
const DIR: &str = "sessions";

/// Tokens and wall time charged by one provider round or tool run.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Usage {
    pub(crate) input: u64,
    pub(crate) output: u64,
    pub(crate) cached: u64,
    pub(crate) millis: u64,
}

impl Usage {
    fn tokens(self) -> u64 {
        self.input.saturating_add(self.output)
    }
}

/// One event in the log.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Line {
    /// First line: who this session is.
    Session {
        id: SessionId,
        model: String,
        system: String,
        workspace: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<SessionId>,
    },
    /// Session ceilings; the last one wins.
    Budget {
        tokens: Option<u64>,
        millis: Option<u64>,
    },
    /// The active workflow; the last one wins, `None` clears it.
    Workflow {
        name: Option<String>,
    },
    /// A user message; `start` marks the one that opens a turn.
    User {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<Image>,
        #[serde(default)]
        start: bool,
    },
    Assistant {
        text: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reasoning: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ToolCall>,
        usage: Usage,
        /// The model that answered.
        #[serde(default)]
        model: String,
    },
    Tool {
        call: String,
        text: String,
        success: bool,
        millis: u64,
    },
    /// Usage with no message: a failed round, the judge, a summary round.
    Charge {
        usage: Usage,
        note: String,
        #[serde(default)]
        model: String,
    },
    /// The visible receipt that replaces the last assistant text.
    Receipt {
        text: String,
    },
    /// Compaction: history restarts at the open turn's opener, this summary
    /// of the older turns, then `tail` — the recent turns kept verbatim.
    Summary {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tail: Vec<Message>,
    },
    Clear,
    /// The open turn ended; `reason` is set when it was aborted.
    Close {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

/// Conversation state folded from the log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) id: Option<SessionId>,
    pub(crate) model: String,
    pub(crate) system: String,
    pub(crate) workspace: PathBuf,
    pub(crate) parent: Option<SessionId>,
    pub(crate) tokens: Option<u64>,
    pub(crate) millis: Option<u64>,
    pub(crate) workflow: Option<String>,
    pub(crate) history: Vec<Message>,
    pub(crate) spent: u64,
    pub(crate) elapsed: u64,
    /// Index of the message that opened the turn, while one is open.
    pub(crate) opener: Option<usize>,
    /// Provider ids of tool calls the open turn has not answered.
    pub(crate) unanswered: Vec<String>,
}

impl State {
    pub(crate) fn apply(&mut self, line: Line) {
        match line {
            Line::Session {
                id,
                model,
                system,
                workspace,
                parent,
            } => {
                self.id = Some(id);
                self.model = model;
                self.system = system;
                self.workspace = workspace;
                self.parent = parent;
            }
            Line::Budget { tokens, millis } => {
                self.tokens = tokens;
                self.millis = millis;
            }
            Line::Workflow { name } => self.workflow = name,
            Line::User {
                text,
                images,
                start,
            } => {
                if start {
                    self.opener = Some(self.history.len());
                    self.unanswered.clear();
                }
                let mut message = Message::user(text);
                message.images = images;
                self.history.push(message);
            }
            Line::Assistant {
                text,
                reasoning,
                tool_calls,
                usage,
                ..
            } => {
                self.charge(usage);
                self.unanswered = tool_calls.iter().map(|call| call.id.clone()).collect();
                self.history.push(Message {
                    role: Role::Assistant,
                    content: text,
                    images: Vec::new(),
                    tool_calls,
                    tool_call_id: None,
                    reasoning: (!reasoning.is_empty()).then_some(reasoning),
                });
            }
            Line::Tool {
                call, text, millis, ..
            } => {
                self.elapsed = self.elapsed.saturating_add(millis);
                self.unanswered.retain(|id| *id != call);
                self.history.push(Message::tool(call, text));
            }
            Line::Charge { usage, .. } => self.charge(usage),
            Line::Receipt { text } => {
                if let Some(last) = self.history.last_mut()
                    && last.role == Role::Assistant
                {
                    last.content = text;
                }
            }
            Line::Summary { text, tail } => {
                let opener = self
                    .opener
                    .and_then(|index| self.history.get(index).cloned());
                self.history.clear();
                self.opener = opener.as_ref().map(|_| 0);
                self.history.extend(opener);
                self.history.push(Message::system(text));
                self.history.extend(tail);
            }
            Line::Clear => {
                self.history.clear();
                self.opener = None;
                self.unanswered.clear();
            }
            Line::Close { .. } => {
                self.opener = None;
                self.unanswered.clear();
            }
        }
    }

    fn charge(&mut self, usage: Usage) {
        self.spent = self.spent.saturating_add(usage.tokens());
        self.elapsed = self.elapsed.saturating_add(usage.millis);
    }

    pub(crate) fn replay(&self) -> Replay {
        Replay {
            history: self.history.clone(),
            prompt: Some(self.system.clone()),
            model: Some(self.model.clone()),
            id: self.id.map(|id| id.to_string()).unwrap_or_default(),
            spent_tokens: self.spent,
            elapsed_millis: self.elapsed,
        }
    }
}

/// Append handle on one session log.
pub(crate) struct Log {
    path: PathBuf,
    file: File,
}

impl Log {
    /// Create the log; fails when the file already exists.
    pub(crate) fn create(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
        })
    }

    /// Reopen an existing log for appending, healing a torn last line first so
    /// the next append cannot weld onto it and brick every later resume.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        heal(path)?;
        let file = OpenOptions::new().append(true).open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
        })
    }

    pub(crate) fn append(&mut self, line: &Line) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(line)?;
        bytes.push(b'\n');
        self.file.write_all(&bytes)?;
        self.file.flush()
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

/// Make the log end at a line boundary. A crash mid-append can leave a tail
/// with no trailing newline; without this the next append concatenates onto it
/// and produces one unparseable line that fails every later read. A tail that
/// is a complete line missing only its newline is kept (the newline is added);
/// a genuinely partial tail is dropped.
fn heal(path: &Path) -> io::Result<()> {
    // Read bytes: a crash can cut the tail mid-UTF-8, and read_to_string would
    // fail on that before this could truncate it, bricking every later open.
    let bytes = fs::read(path)?;
    if bytes.is_empty() || bytes.last() == Some(&b'\n') {
        return Ok(());
    }
    let complete = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let whole = std::str::from_utf8(&bytes[complete..])
        .ok()
        .is_some_and(|tail| serde_json::from_str::<Line>(tail.trim()).is_ok());
    if whole {
        OpenOptions::new().append(true).open(path)?.write_all(b"\n")
    } else {
        OpenOptions::new()
            .write(true)
            .open(path)?
            .set_len(complete as u64)
    }
}

/// Every complete line in the log, in order.
pub(crate) fn read(path: &Path) -> io::Result<Vec<Line>> {
    // Complete lines end at the last newline and are whole UTF-8; only the
    // torn tail after it can be cut mid-char. Convert the prefix strictly and
    // the tail leniently so a mid-UTF-8 crash never fails the whole read.
    let bytes = fs::read(path)?;
    let complete = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let text = std::str::from_utf8(&bytes[..complete])
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        lines.push(serde_json::from_str(line)?);
    }
    if let Ok(tail) = std::str::from_utf8(&bytes[complete..])
        && !tail.trim().is_empty()
        && let Ok(line) = serde_json::from_str(tail)
    {
        lines.push(line);
    }
    Ok(lines)
}

/// The header line alone, without reading the rest of the log.
pub(crate) fn header(path: &Path) -> io::Result<Line> {
    // The header is the first, always-complete line. Read bytes and stop at
    // the first newline so a torn multibyte tail later in the file cannot fail
    // the strict UTF-8 conversion.
    let bytes = fs::read(path)?;
    let end = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(bytes.len());
    let first = std::str::from_utf8(&bytes[..end])
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(serde_json::from_str(first)?)
}

pub(crate) fn session_path(home: &Path, session: SessionId) -> PathBuf {
    home.join(DIR).join(session.to_string()).join(FILE)
}

/// The checkpoint file beside a session log.
pub(crate) fn work_path(session: &Path) -> PathBuf {
    session.with_file_name(WORK)
}

/// The `sessions` directory under a home.
pub(crate) fn sessions_dir(home: &Path) -> PathBuf {
    home.join(DIR)
}

/// The session id a log path names, when it lives under `sessions/`.
pub(crate) fn parse_path(path: &Path) -> Option<SessionId> {
    if path.file_name()?.to_str()? != FILE {
        return None;
    }
    let dir = path.parent()?;
    if dir.parent()?.file_name()?.to_str()? != DIR {
        return None;
    }
    dir.file_name()?.to_str()?.parse().ok()
}

/// The home a session log lives under.
pub(crate) fn home_of(path: &Path) -> Option<&Path> {
    path.parent()?.parent()?.parent()
}
