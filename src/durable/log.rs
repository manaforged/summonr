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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Line {
    Session {
        id: SessionId,
        model: String,
        system: String,
        workspace: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<SessionId>,
        #[serde(default, skip_serializing_if = "is_zero")]
        depth: u8,
    },
    Budget {
        tokens: Option<u64>,
        millis: Option<u64>,
    },
    Workflow {
        name: Option<String>,
    },
    Child {
        record: crate::child::ChildRecord,
    },
    User {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<String>,
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
        #[serde(default)]
        model: String,
    },
    Interrupted {
        text: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reasoning: String,
    },
    Tool {
        call: String,
        text: String,
        success: bool,
        millis: u64,
    },
    Charge {
        usage: Usage,
        note: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        reported: bool,
    },
    Receipt {
        text: String,
    },
    Summary {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tail: Vec<Message>,
    },
    Clear,
    Close {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) id: Option<SessionId>,
    pub(crate) model: String,
    pub(crate) system: String,
    pub(crate) workspace: PathBuf,
    pub(crate) parent: Option<SessionId>,
    pub(crate) depth: u8,
    pub(crate) tokens: Option<u64>,
    pub(crate) millis: Option<u64>,
    pub(crate) workflow: Option<String>,
    pub(crate) children: std::collections::BTreeMap<SessionId, crate::child::ChildRecord>,
    pub(crate) history: Vec<Message>,
    pub(crate) spent: u64,
    pub(crate) unreported: u64,
    pub(crate) elapsed: u64,
    pub(crate) opener: Option<usize>,
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
                depth,
            } => {
                self.id = Some(id);
                self.model = model;
                self.system = system;
                self.workspace = workspace;
                self.parent = parent;
                self.depth = depth;
            }
            Line::Budget { tokens, millis } => {
                self.tokens = tokens;
                self.millis = millis;
            }
            Line::Workflow { name } => self.workflow = name,
            Line::Child { record } => {
                self.children.insert(record.id, record);
            }
            Line::User {
                text,
                images,
                start,
                ..
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
            Line::Interrupted { text, .. } => {
                if !text.is_empty() {
                    self.history.push(Message {
                        role: Role::Assistant,
                        content: text,
                        images: Vec::new(),
                        tool_calls: Vec::new(),
                        tool_call_id: None,
                        reasoning: None,
                    });
                }
            }
            Line::Tool {
                call, text, millis, ..
            } => {
                self.elapsed = self.elapsed.saturating_add(millis);
                self.unanswered.retain(|id| *id != call);
                self.history.push(Message::tool(call, text));
            }
            Line::Charge {
                usage, reported, ..
            } => {
                if !reported {
                    self.unreported = self.unreported.saturating_add(usage.tokens());
                }
                self.charge(usage);
            }
            Line::Receipt { text } => {
                self.history.push(Message::system(text));
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

pub(crate) struct Log {
    path: PathBuf,
    file: File,
}

impl Log {
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

    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        heal(path)?;
        let file = OpenOptions::new().append(true).open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file,
        })
    }

    pub(crate) fn append(&mut self, line: &Line) -> io::Result<()> {
        let mut value = serde_json::to_value(line)?;
        if let Some(object) = value.as_object_mut() {
            object.insert("at".to_string(), serde_json::Value::from(now_millis()));
        }
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.file.write_all(&bytes)?;
        self.file.flush()
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

fn heal(path: &Path) -> io::Result<()> {
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

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

pub(crate) fn read(path: &Path) -> io::Result<Vec<Line>> {
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

pub(crate) fn header(path: &Path) -> io::Result<Line> {
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

pub(crate) fn work_path(session: &Path) -> PathBuf {
    session.with_file_name(WORK)
}

pub(crate) fn name_path(session: &Path) -> PathBuf {
    session.with_file_name("name")
}

pub(crate) fn sessions_dir(home: &Path) -> PathBuf {
    home.join(DIR)
}

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

pub(crate) fn home_of(path: &Path) -> Option<&Path> {
    path.parent()?.parent()?.parent()
}
