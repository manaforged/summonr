use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::id::SessionId;
use crate::provider::{Continuation, Image, Message, Role, ToolCall};
use crate::session::Replay;

const FILE: &str = "session.jsonl";
const WORK: &str = "work.json";
const DIR: &str = "sessions";

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Usage {
    pub(crate) input: u64,
    pub(crate) output: u64,
    #[serde(default)]
    pub(crate) reasoning: u64,
    pub(crate) cached: u64,
    pub(crate) millis: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct UsageMeta {
    pub(crate) prefix_hash: String,
    pub(crate) cost: Option<f64>,
    pub(crate) continuation: Option<Continuation>,
}

impl Usage {
    fn tokens(self) -> u64 {
        self.input.saturating_add(self.output)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
    Tools {
        bytes: u64,
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
        #[serde(default, skip_serializing_if = "String::is_empty")]
        prefix_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        continuation: Option<Continuation>,
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
        #[serde(default, skip_serializing_if = "is_false")]
        mutated: bool,
        millis: u64,
    },
    Charge {
        usage: Usage,
        note: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        reported: bool,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        prefix_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost: Option<f64>,
    },
    Receipt {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        receipt: Option<crate::verify::Receipt>,
    },
    Summary {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tail: Vec<Message>,
    },
    Rewrite {
        edits: Vec<(usize, Message)>,
    },
    Clear,
    Close {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", alias = "rss_kb")]
        footprint_kb: Option<u64>,
    },
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
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
            Line::Tools { .. } | Line::Receipt { .. } => {}
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
                continuation,
                ..
            } => {
                self.charge(usage);
                self.unanswered = tool_calls.iter().map(|call| call.id.clone()).collect();
                self.history.push(Message {
                    role: Role::Assistant,
                    tool_calls,
                    reasoning: (!reasoning.is_empty()).then_some(reasoning),
                    continuation,
                    ..Message::system(text)
                });
            }
            Line::Interrupted { text, .. } => {
                self.history.extend((!text.is_empty()).then(|| Message {
                    role: Role::Assistant,
                    ..Message::system(text)
                }));
            }
            Line::Tool {
                call,
                text,
                success,
                millis,
                ..
            } => {
                self.elapsed = self.elapsed.saturating_add(millis);
                self.unanswered.retain(|id| *id != call);
                self.history.push(Message {
                    failed: !success,
                    ..Message::tool(call, text)
                });
            }
            Line::Charge {
                usage, reported, ..
            } => {
                if !reported {
                    self.unreported = self.unreported.saturating_add(usage.tokens());
                }
                self.charge(usage);
            }
            Line::Summary { text, tail } => {
                let opener = self
                    .opener
                    .and_then(|index| self.history.get(index).cloned());
                self.history.clear();
                self.opener = opener.as_ref().map(|_| 0);
                self.history.extend(opener.clone());
                self.history.push(Message::system(text));
                let tail = if tail.first().is_some_and(|m| Some(m) == opener.as_ref()) {
                    &tail[1..]
                } else {
                    &tail[..]
                };
                self.history.extend(tail.iter().cloned());
            }
            Line::Rewrite { mut edits } => {
                edits.retain(|(index, _)| *index < self.history.len());
                for (index, message) in edits {
                    self.history[index] = message;
                }
            }
            end @ (Line::Clear | Line::Close { .. }) => {
                if end == Line::Clear {
                    self.history.clear();
                }
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
        #[derive(Serialize)]
        struct At<'a> {
            at: u64,
            #[serde(flatten)]
            line: &'a Line,
        }
        let mut bytes = serde_json::to_vec(&At {
            at: now_millis(),
            line,
        })?;
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

mod paths;

pub(crate) use paths::*;

#[cfg(test)]
#[path = "log_tests.rs"]
mod tests;
