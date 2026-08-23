use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Error as IoError, ErrorKind as IoKind, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::result::Result as StdResult;

use thiserror::Error;

use super::encoding::{Commit, EncodingError};
use super::fact::{CommandReceipt, Fact, FactData};
use super::id::{CommandId, CommitId, Digest, OperationId, SessionId};
use super::reducer::{ReduceError, SessionState, apply, prepare};

#[derive(Debug, Error)]
pub(crate) enum JournalError {
    #[error("journal I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: IoError,
    },
    #[error("journal {path} is already owned by another writer: {detail}")]
    Locked { path: PathBuf, detail: String },
    #[error("journal line {line} is invalid: {source}")]
    Encoding {
        line: usize,
        #[source]
        source: EncodingError,
    },
    #[error("journal line {line} cannot reduce: {source}")]
    Reduce {
        line: usize,
        #[source]
        source: ReduceError,
    },
    #[error("journal writer is poisoned after an append failure")]
    Poisoned,
}

pub(crate) type Result<T> = StdResult<T, JournalError>;

pub(crate) struct JournalWriter {
    path: PathBuf,
    file: File,
    _lock: File,
    state: SessionState,
    poisoned: bool,
    #[cfg(test)]
    fail_after: Option<usize>,
}

impl JournalWriter {
    pub(crate) fn create(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        make_parent(&path)?;
        let lock = lock(&path)?;
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| io(&path, source))?;
        Ok(Self {
            path,
            file,
            _lock: lock,
            state: SessionState::default(),
            poisoned: false,
            #[cfg(test)]
            fail_after: None,
        })
    }

    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let lock = lock(&path)?;
        let bytes = fs::read(&path).map_err(|source| io(&path, source))?;
        let (state, complete) = replay(&bytes)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| io(&path, source))?;
        if complete != bytes.len() {
            file.set_len(
                u64::try_from(complete)
                    .map_err(|source| io(&path, IoError::new(IoKind::InvalidData, source)))?,
            )
            .map_err(|source| io(&path, source))?;
            file.sync_data().map_err(|source| io(&path, source))?;
        }
        file.seek(SeekFrom::End(0))
            .map_err(|source| io(&path, source))?;
        Ok(Self {
            path,
            file,
            _lock: lock,
            state,
            poisoned: false,
            #[cfg(test)]
            fail_after: None,
        })
    }

    pub(crate) fn append(
        &mut self,
        id: CommitId,
        command: Option<CommandReceipt>,
        facts: Vec<Fact>,
    ) -> Result<Digest> {
        if self.poisoned {
            return Err(JournalError::Poisoned);
        }
        if let Some(receipt) = &command
            && self.receipt(receipt.id)?.is_some()
        {
            return Err(JournalError::Reduce {
                line: usize::try_from(self.state.seq().saturating_add(1)).unwrap_or(usize::MAX),
                source: ReduceError::Duplicate { kind: "command" },
            });
        }
        let commit = Commit::new(
            self.state.seq().saturating_add(1),
            id,
            self.state.head(),
            command,
            facts,
        )
        .map_err(|source| JournalError::Encoding {
            line: usize::try_from(self.state.seq().saturating_add(1)).unwrap_or(usize::MAX),
            source,
        })?;
        if let Some(operation) = opened(&commit)
            && self.closed(operation)?
        {
            return Err(JournalError::Reduce {
                line: usize::try_from(self.state.seq().saturating_add(1)).unwrap_or(usize::MAX),
                source: ReduceError::Duplicate { kind: "operation" },
            });
        }
        let prepared = prepare(&self.state, commit).map_err(|source| JournalError::Reduce {
            line: usize::try_from(self.state.seq().saturating_add(1)).unwrap_or(usize::MAX),
            source,
        })?;
        let line = prepared
            .commit()
            .line()
            .map_err(|source| JournalError::Encoding {
                line: usize::try_from(self.state.seq().saturating_add(1)).unwrap_or(usize::MAX),
                source,
            })?;
        #[cfg(test)]
        let result = if let Some(length) = self.fail_after.take() {
            self.file
                .write_all(&line[..length.min(line.len())])
                .and_then(|()| self.file.sync_data())
                .and_then(|()| Err(IoError::other("injected partial append")))
        } else {
            self.file
                .write_all(&line)
                .and_then(|()| self.file.sync_data())
        };
        #[cfg(not(test))]
        let result = self
            .file
            .write_all(&line)
            .and_then(|()| self.file.sync_data());
        if let Err(source) = result {
            self.poisoned = true;
            return Err(io(&self.path, source));
        }
        let digest = prepared.commit().digest;
        apply(&mut self.state, prepared);
        Ok(digest)
    }

    pub(crate) fn state(&self) -> &SessionState {
        &self.state
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn receipt(&self, id: CommandId) -> Result<Option<CommandReceipt>> {
        if let Some(receipt) = self.state.receipt(id) {
            return Ok(Some(receipt.clone()));
        }
        if !self.state.maybe_receipt(id) {
            return Ok(None);
        }
        find_receipt(&self.path, id)
    }

    fn closed(&self, id: OperationId) -> Result<bool> {
        if self.state.closed_operation(id).is_some() {
            return Ok(true);
        }
        if !self.state.maybe_closed(id) {
            return Ok(false);
        }
        let bytes = fs::read(&self.path).map_err(|source| io(&self.path, source))?;
        scan_closed(&bytes, id)
    }

    #[cfg(test)]
    pub(crate) fn fail_after(&mut self, bytes: usize) {
        self.fail_after = Some(bytes);
    }
}

pub(crate) fn inspect(path: impl AsRef<Path>) -> Result<SessionState> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|source| io(path, source))?;
    replay(&bytes).map(|(state, _)| state)
}

pub(crate) fn find_receipt(
    path: impl AsRef<Path>,
    id: CommandId,
) -> Result<Option<CommandReceipt>> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|source| io(path, source))?;
    replay_inner(&bytes, Some(id)).map(|(_, _, receipt)| receipt)
}

pub(crate) fn session_path(home: &Path, session: SessionId) -> PathBuf {
    home.join("sessions-v2")
        .join(format!("{}.jsonl", session.path_component()))
}

pub(crate) fn parse_path(path: &Path) -> Option<SessionId> {
    if path.extension().and_then(|value| value.to_str()) != Some("jsonl")
        || path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            != Some("sessions-v2")
    {
        return None;
    }
    path.file_stem()
        .and_then(|value| value.to_str())
        .and_then(|value| value.parse().ok())
}

fn replay(bytes: &[u8]) -> Result<(SessionState, usize)> {
    replay_inner(bytes, None).map(|(state, complete, _)| (state, complete))
}

fn replay_inner(
    bytes: &[u8],
    wanted: Option<CommandId>,
) -> Result<(SessionState, usize, Option<CommandReceipt>)> {
    let complete = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let mut state = SessionState::default();
    let mut start = 0;
    let mut line_number = 1;
    let mut found = None;
    while start < complete {
        let end = bytes[start..complete]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(complete, |offset| start + offset);
        let line = &bytes[start..end];
        if line.is_empty() {
            return Err(JournalError::Encoding {
                line: line_number,
                source: EncodingError::Empty,
            });
        }
        let commit = Commit::parse(line).map_err(|source| JournalError::Encoding {
            line: line_number,
            source,
        })?;
        if let Some(operation) = opened(&commit)
            && (state.closed_operation(operation).is_some()
                || (state.maybe_closed(operation) && scan_closed(&bytes[..start], operation)?))
        {
            return Err(JournalError::Reduce {
                line: line_number,
                source: ReduceError::Duplicate { kind: "operation" },
            });
        }
        if let Some(receipt) = &commit.command {
            if state.receipt(receipt.id).is_none()
                && state.maybe_receipt(receipt.id)
                && scan_receipt(&bytes[..start], receipt.id)?.is_some()
            {
                return Err(JournalError::Reduce {
                    line: line_number,
                    source: ReduceError::Duplicate { kind: "command" },
                });
            }
            if wanted == Some(receipt.id) {
                found = Some(receipt.clone());
            }
        }
        let prepared = prepare(&state, commit).map_err(|source| JournalError::Reduce {
            line: line_number,
            source,
        })?;
        apply(&mut state, prepared);
        start = end.saturating_add(1);
        line_number += 1;
    }
    Ok((state, complete, found))
}

fn scan_receipt(bytes: &[u8], id: CommandId) -> Result<Option<CommandReceipt>> {
    scan(bytes, |commit| {
        commit
            .command
            .as_ref()
            .filter(|receipt| receipt.id == id)
            .cloned()
    })
}

fn scan_closed(bytes: &[u8], id: OperationId) -> Result<bool> {
    scan(bytes, |commit| {
        commit.facts.iter().find_map(|fact| match &fact.data {
            FactData::OperationClosed { close, .. } if close.id == id => Some(()),
            _ => None,
        })
    })
    .map(|found| found.is_some())
}

fn scan<T>(bytes: &[u8], mut find: impl FnMut(&Commit) -> Option<T>) -> Result<Option<T>> {
    let mut start = 0;
    let mut line_number = 1;
    while start < bytes.len() {
        let end = bytes[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |offset| start + offset);
        let commit =
            Commit::parse(&bytes[start..end]).map_err(|source| JournalError::Encoding {
                line: line_number,
                source,
            })?;
        if let Some(found) = find(&commit) {
            return Ok(Some(found));
        }
        start = end.saturating_add(1);
        line_number += 1;
    }
    Ok(None)
}

fn opened(commit: &Commit) -> Option<OperationId> {
    commit.facts.iter().find_map(|fact| match &fact.data {
        FactData::OperationOpened { operation, .. } => Some(*operation),
        _ => None,
    })
}

fn lock(path: &Path) -> Result<File> {
    let sidecar = lock_path(path);
    make_parent(&sidecar)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&sidecar)
        .map_err(|source| io(&sidecar, source))?;
    lock.try_lock().map_err(|error| JournalError::Locked {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    Ok(lock)
}

fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(OsString::from(".lock"));
    PathBuf::from(name)
}

fn make_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| io(parent, source))?;
    }
    Ok(())
}

fn io(path: &Path, source: IoError) -> JournalError {
    JournalError::Io {
        path: path.to_path_buf(),
        source,
    }
}
