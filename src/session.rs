//! Append-only session log. Resume is a replay of this file.

use std::fs::{File, OpenOptions, create_dir_all};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::prompt::mage_home;
use crate::provider::{Image, Message, Role, ToolCall, ToolFunction, ToolKind, UserInput};
use crate::tools::Event;

mod catalog;

pub use catalog::{last_for, list, resolve_id};

/// One listed session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub path: PathBuf,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub preview: String,
}

/// History reconstructed from a log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replay {
    pub history: Vec<Message>,
    pub task: Option<crate::task::TaskState>,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub id: String,
    pub spent_tokens: u64,
}

/// Durable JSONL log for one conversation.
pub struct SessionLog {
    id: String,
    path: PathBuf,
    file: File,
    seq: u64,
}

impl SessionLog {
    /// Create `$MAGE_HOME/sessions/<id>.jsonl` and write a header.
    pub fn create(cwd: &Path, model: &str, prompt: &str) -> Result<Self> {
        let dir = sessions_dir()?;
        create_dir_all(&dir)?;
        let id = new_id()?;
        let path = dir.join(format!("{id}.jsonl"));
        let mut log = Self::open_append(path, 0, id.clone())?;
        log.write(&Record::Header {
            id: id.clone(),
            cwd: cwd.display().to_string(),
            model: model.to_string(),
            prompt: prompt.to_string(),
            parent: None,
        })?;
        Ok(log)
    }

    /// Fork a session: a new log carrying the source's records verbatim,
    /// headed by a parent pointer. Returns the open log and its replay.
    pub fn fork(source: &Path) -> Result<(Self, Replay)> {
        let dir = sessions_dir()?;
        Self::fork_to(source, &dir)
    }

    /// Fork into a specific directory (tests keep their own).
    pub(crate) fn fork_to(source: &Path, dir: &Path) -> Result<(Self, Replay)> {
        let (source_log, _) = Self::resume(source)?;
        drop(source_log);
        let text = std::fs::read_to_string(source)?;
        let header = text.lines().next().unwrap_or("");
        let (cwd, model, prompt, parent) = match parse_record(header)? {
            Record::Header {
                id,
                cwd,
                model,
                prompt,
                ..
            } => (cwd, model, prompt, id),
            _ => {
                return Err(Error::Config(
                    "session log is missing its header".to_string(),
                ));
            }
        };
        create_dir_all(dir)?;
        let id = new_id()?;
        let path = dir.join(format!("{id}.jsonl"));
        let mut log = Self::open_append(path, 0, id.clone())?;
        log.write(&Record::Header {
            id: id.clone(),
            cwd,
            model,
            prompt,
            parent: Some(parent),
        })?;
        for line in text.lines().skip(1) {
            if line.trim().is_empty() {
                continue;
            }
            log.file.write_all(line.as_bytes())?;
            log.file.write_all(b"\n")?;
        }
        log.file.flush()?;
        log.seq = count_seq(&log.path)?;
        let replay = replay(&log.path)?;
        Ok((log, replay))
    }

    /// Open (or create) a specific log path. Used by tests.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("session")
            .to_string();
        Self::open_append(path, 0, id)
    }

    /// Open an existing log for append after a replay.
    pub fn resume(path: impl AsRef<Path>) -> Result<(Self, Replay)> {
        let path = path.as_ref().to_path_buf();
        let bytes = std::fs::read(&path)?;
        let terminated = bytes.last().is_none_or(|byte| *byte == b'\n');
        let replay = match replay(&path) {
            Ok(replay) => {
                if !terminated {
                    OpenOptions::new()
                        .append(true)
                        .open(&path)?
                        .write_all(b"\n")?;
                }
                replay
            }
            Err(error) if !terminated => {
                let Some(end) = bytes.iter().rposition(|byte| *byte == b'\n') else {
                    return Err(error);
                };
                OpenOptions::new().write(true).open(&path)?.set_len(
                    u64::try_from(end + 1).map_err(|error| {
                        Error::Config(format!("session log is too large to recover: {error}"))
                    })?,
                )?;
                replay(&path)?
            }
            Err(error) => return Err(error),
        };
        let seq = count_seq(&path)?;
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("session")
            .to_string();
        let log = Self::open_append(path, seq, id)?;
        Ok((log, replay))
    }

    fn open_append(path: PathBuf, seq: u64, id: String) -> Result<Self> {
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            id,
            path,
            file,
            seq,
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn append_user(&mut self, text: &str) -> Result<()> {
        self.append_input(&UserInput::from(text))
    }

    pub(crate) fn append_input(&mut self, input: &UserInput) -> Result<()> {
        self.append_input_with_harness(input, None)
    }

    pub(crate) fn append_input_with_harness(
        &mut self,
        input: &UserInput,
        harness: Option<&str>,
    ) -> Result<()> {
        self.write(&Record::User {
            text: input.text.clone(),
            images: input.images.clone(),
            harness: harness.map(str::to_string),
        })
    }

    pub fn append_event(&mut self, event: &Event) -> Result<()> {
        self.write(&Record::Event {
            event: event.clone(),
        })
    }

    /// Retain provider reasoning for replay without emitting a public event.
    #[allow(dead_code, reason = "v1 replay still deserializes reasoning records")]
    pub(crate) fn append_reasoning(&mut self, text: &str) -> Result<()> {
        self.write(&Record::Reasoning {
            text: text.to_string(),
        })
    }

    /// Record a compaction checkpoint: replay replaces everything before
    /// it with the summary.
    pub fn append_compaction(&mut self, summary: &str, keep: usize) -> Result<()> {
        self.write(&Record::Compaction {
            summary: summary.to_string(),
            keep,
        })
    }

    /// Record a conversation reset. Replay discards all prior history.
    pub fn append_clear(&mut self) -> Result<()> {
        self.write(&Record::Clear)
    }

    fn write(&mut self, record: &Record) -> Result<()> {
        self.seq += 1;
        let line = Line {
            seq: self.seq,
            record: record.clone(),
        };
        let encoded = serde_json::to_string(&line)?;
        self.file.write_all(encoded.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.flush()?;
        Ok(())
    }
}

pub(crate) fn sessions_dir() -> Result<PathBuf> {
    Ok(mage_home()?.join("sessions"))
}

/// Rebuild provider history from a log.
pub fn replay(path: &Path) -> Result<Replay> {
    let file = File::open(path)?;
    let mut replay = Replay {
        id: path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("session")
            .to_string(),
        ..Replay::default()
    };
    let mut pending: Vec<ToolCall> = Vec::new();
    let mut pending_results: Vec<(String, String)> = Vec::new();
    let mut pending_reasoning = None;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record = parse_record(&line)?;
        match record {
            Record::Header {
                id, model, prompt, ..
            } => {
                replay.id = id;
                replay.model = Some(model);
                replay.prompt = Some(prompt);
            }
            Record::User {
                text,
                images,
                harness,
            } => {
                pending.clear();
                pending_results.clear();
                pending_reasoning = None;
                let mut message = Message::user(UserInput { text, images });
                if let Some(harness) = harness {
                    message.content = decorate(&harness, &message.content);
                }
                replay.history.push(message);
            }
            Record::Compaction { summary, keep } => {
                pending.clear();
                pending_results.clear();
                pending_reasoning = None;
                let tail = replay
                    .history
                    .split_off(replay.history.len().saturating_sub(keep));
                replay.history = vec![Message::user(format!(
                    "[summon compacted the earlier context; summary follows]\n{summary}"
                ))];
                replay.history.extend(tail);
            }
            Record::Clear => {
                pending.clear();
                pending_results.clear();
                pending_reasoning = None;
                replay.history.clear();
                replay.task = None;
            }
            Record::Reasoning { text } => pending_reasoning = Some(text),
            Record::Event { event } => apply_event(
                &mut replay.history,
                &mut pending,
                &mut pending_results,
                &mut pending_reasoning,
                &mut replay.spent_tokens,
                &mut replay.task,
                event,
            ),
        }
    }
    Ok(replay)
}

/// Replay a v1 log and report unmatched tool-call ids still open at the end.
pub(crate) fn replay_pending(path: &Path) -> Result<(Replay, Vec<String>)> {
    let replayed = replay(path)?;
    let file = File::open(path)?;
    let mut pending: Vec<String> = Vec::new();
    let mut pending_results = 0_usize;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(record) = parse_record(&line) else {
            continue;
        };
        match record {
            Record::User { .. } | Record::Compaction { .. } | Record::Clear => {
                pending.clear();
                pending_results = 0;
            }
            Record::Event {
                event: Event::ToolCall { id, .. },
            } => pending.push(id),
            Record::Event {
                event: Event::ToolResult { .. },
            } => {
                pending_results += 1;
                if pending_results == pending.len() {
                    pending.clear();
                    pending_results = 0;
                }
            }
            Record::Event {
                event: Event::Assistant { .. },
            } => {
                pending.clear();
                pending_results = 0;
            }
            _ => {}
        }
    }
    Ok((replayed, pending))
}

fn apply_event(
    history: &mut Vec<Message>,
    pending: &mut Vec<ToolCall>,
    pending_results: &mut Vec<(String, String)>,
    pending_reasoning: &mut Option<String>,
    spent_tokens: &mut u64,
    task: &mut Option<crate::task::TaskState>,
    event: Event,
) {
    match event {
        Event::ToolCall {
            id,
            name,
            arguments,
        } => {
            pending.push(ToolCall {
                id,
                kind: ToolKind::Function,
                function: ToolFunction { name, arguments },
            });
        }
        Event::ToolResult {
            id,
            output,
            task: next_task,
            ..
        } => {
            if next_task.is_some() {
                *task = next_task;
            }
            pending_results.push((id, output));
            if !pending.is_empty() && pending_results.len() == pending.len() {
                history.push(Message {
                    role: Role::Assistant,
                    content: String::new(),
                    images: Vec::new(),
                    tool_calls: std::mem::take(pending),
                    tool_call_id: None,
                    reasoning: pending_reasoning.take(),
                });
                for (id, output) in pending_results.drain(..) {
                    history.push(Message::tool(id, output));
                }
            }
        }
        Event::Assistant { text } => {
            pending.clear();
            pending_results.clear();
            history.push(Message {
                role: Role::Assistant,
                content: text,
                images: Vec::new(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                reasoning: pending_reasoning.take(),
            });
        }
        Event::Usage {
            prompt_tokens,
            completion_tokens,
            ..
        } => {
            *spent_tokens = spent_tokens
                .saturating_add(u64::from(prompt_tokens))
                .saturating_add(u64::from(completion_tokens));
        }
        Event::Mail { from, text } => {
            history.push(Message::user(crate::bus::envelope(&from, &text)));
        }
        Event::Aborted { .. } | Event::Delta { .. } | Event::Note { .. } | Event::Verify { .. } => {
        }
    }
}

fn count_seq(path: &Path) -> Result<u64> {
    let file = File::open(path)?;
    let mut seq = 0_u64;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line)
            && let Some(n) = value.get("seq").and_then(serde_json::Value::as_u64)
        {
            seq = seq.max(n);
        } else {
            seq += 1;
        }
    }
    Ok(seq)
}

fn parse_record(line: &str) -> Result<Record> {
    if let Ok(line) = serde_json::from_str::<Line>(line) {
        return Ok(line.record);
    }
    serde_json::from_str::<Record>(line)
        .map_err(|error| Error::Config(format!("session log is invalid: {error}")))
}

fn new_id() -> Result<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Config(error.to_string()))?
        .as_nanos();
    Ok(format!("{nanos}-{}", std::process::id()))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Line {
    seq: u64,
    #[serde(flatten)]
    record: Record,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Record {
    Header {
        id: String,
        cwd: String,
        model: String,
        prompt: String,
        /// The session this log forked from, when it did.
        #[serde(default)]
        parent: Option<String>,
    },
    User {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<Image>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        harness: Option<String>,
    },
    Event {
        event: Event,
    },
    /// Model reasoning for the next assistant message in the log.
    Reasoning {
        text: String,
    },
    /// A compaction checkpoint: on replay, history before this record
    /// is replaced by the summary.
    Compaction {
        summary: String,
        #[serde(default)]
        keep: usize,
    },
    Clear,
}

pub(crate) fn decorate(harness: &str, text: &str) -> String {
    if text.is_empty() {
        harness.to_string()
    } else {
        format!("{text}\n\n{harness}")
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
