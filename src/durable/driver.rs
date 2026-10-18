use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

use super::id::{Attempt, Call, SessionId};
use super::log::{self, Line, Log, State, Usage, UsageMeta};
use super::tree::{self, Snapshot};
use super::work::{Check, Decision, Policy, Stage, Work, WorkError};
use crate::WorkProgress;
use crate::provider::{Message, Partial, ToolCall, UserInput};
use crate::session::Replay;

#[derive(Debug, Error)]
pub(crate) enum DriverError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Work(#[from] WorkError),
    #[error("input is empty")]
    Empty,
    #[error("a turn is already open")]
    Busy,
    #[error("no open turn")]
    NoTurn,
    #[error("no open provider round")]
    NoProvider,
    #[error("no planned tool for this call")]
    NoTool,
    #[error("session has no Work")]
    NoWork,
    #[error("{0}")]
    State(String),
}

pub(crate) struct Exposure {
    pub(crate) max_input_tokens: u64,
    pub(crate) max_output_tokens: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    Agent,
    Summary,
    Judge,
    Review,
    Scout,
}

struct Round {
    attempt: Attempt,
    purpose: Purpose,
    exposure: Exposure,
    model: String,
}

struct Turn {
    stage: Option<Stage>,
    round: Option<Round>,
    calls: Vec<(Call, String)>,
}

pub(crate) struct Verdict {
    pub(crate) passed: bool,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) struct SessionDriver {
    pub(crate) partial: Partial,
    log: Log,
    state: State,
    work: Option<Work>,
    workspace: PathBuf,
    turn: Option<Turn>,
    next: u64,
    pending: Vec<Line>,
    saved: AtomicU64,
    base: Option<base::Pending>,
    fresh: bool,
    last: Option<Snapshot>,
    notice: Option<String>,
}

mod base;
mod children;
mod session;
mod turns;
mod verdict;
mod work;

impl SessionDriver {
    pub(crate) fn charge(
        &mut self,
        usage: Usage,
        note: &str,
        model: &str,
        reported: bool,
        meta: UsageMeta,
    ) -> Result<(), DriverError> {
        self.append(Line::Charge {
            reported,
            usage,
            note: note.to_string(),
            model: model.to_string(),
            prefix_hash: meta.prefix_hash,
            cost: meta.cost,
        })
    }

    pub(crate) fn turn_open(&self) -> bool {
        self.turn.is_some()
    }

    pub(crate) fn usage(&self) -> (u64, u64) {
        (self.state.spent, self.state.unreported)
    }

    pub(crate) fn compact(&mut self, summary: &str, tail: &[Message]) -> Result<(), DriverError> {
        self.append(Line::Summary {
            text: summary.to_string(),
            tail: tail.to_vec(),
        })
    }

    pub(crate) fn rewrite(&mut self, edits: Vec<(usize, Message)>) -> Result<(), DriverError> {
        self.append(Line::Rewrite { edits })
    }

    pub(crate) fn clear(&mut self) -> Result<(), DriverError> {
        if self.turn.is_some() {
            return Err(DriverError::Busy);
        }
        if let Some(work) = self.work.as_mut().filter(|work| !work.todos.is_empty()) {
            work.todos.clear();
            self.checkpoint()?;
        }
        self.append(Line::Clear)
    }

    pub(crate) fn note_schema(&mut self, bytes: u64) -> Result<(), DriverError> {
        self.append(Line::Tools { bytes })
    }

    pub(crate) fn set_budget(
        &mut self,
        tokens: Option<u64>,
        millis: Option<u64>,
    ) -> Result<(), DriverError> {
        if (self.state.tokens, self.state.millis) == (tokens, millis) {
            return Ok(());
        }
        self.append(Line::Budget { tokens, millis })
    }

    pub(crate) fn budget_limits(&self) -> (Option<u64>, Option<u64>) {
        (self.state.tokens, self.state.millis)
    }

    pub(crate) fn set_workflow(&mut self, name: Option<&str>) -> Result<(), DriverError> {
        if self.state.workflow.as_deref() == name {
            return Ok(());
        }
        self.append(Line::Workflow {
            name: name.map(str::to_string),
        })
    }

    pub(crate) fn workflow(&self) -> Option<&str> {
        self.state.workflow.as_deref()
    }

    pub(crate) fn stage(&self) -> Option<Stage> {
        self.turn.as_ref().and_then(|turn| turn.stage)
    }

    pub(crate) fn replay(&self) -> Replay {
        self.state.replay()
    }

    pub(crate) fn history(&self) -> &[Message] {
        &self.state.history
    }

    pub(crate) fn opener(&self) -> Option<usize> {
        self.state.opener
    }

    pub(crate) fn spent_tokens(&self) -> u64 {
        self.state.spent
    }

    pub(crate) fn elapsed_millis(&self) -> u64 {
        self.state.elapsed
    }

    pub(crate) fn last_close_reason(&self) -> Option<&str> {
        self.state.last_close_reason.as_deref()
    }

    pub(crate) fn path(&self) -> &Path {
        self.log.path()
    }

    pub(crate) fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub(crate) fn id(&self) -> SessionId {
        self.state.id.expect("an open session has a header")
    }

    pub(crate) fn depth(&self) -> u8 {
        self.state.depth
    }

    fn execute(&mut self) -> Result<(), DriverError> {
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .execute(None)?;
        self.begin_base();
        self.checkpoint()
    }

    fn tree_cache(&self) -> Option<PathBuf> {
        log::home_of(self.log.path()).map(|home| tree::cache_dir(&canonical(home)))
    }

    fn excludes(&self) -> Vec<PathBuf> {
        self.log
            .path()
            .parent()
            .and_then(Path::parent)
            .map(canonical)
            .into_iter()
            .chain(self.tree_cache())
            .collect()
    }

    fn remember_tree(&mut self) {
        if let (Some(cache), Some(last)) = (self.tree_cache(), self.last.as_mut()) {
            tree::remember(&cache, &self.workspace, last);
        }
    }

    fn snapshot(&mut self, previous: Option<&Snapshot>) -> Result<Snapshot, DriverError> {
        let excludes = self.excludes();
        let previous = previous.or(self.last.as_ref());
        let capture = || tree::snapshot(&self.workspace, &excludes, previous);
        let threaded = tokio::runtime::Handle::try_current().is_ok_and(|handle| {
            handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread
        });
        let snapshot = if threaded {
            tokio::task::block_in_place(capture)
        } else {
            capture()
        }?;
        self.last = Some(snapshot.clone());
        Ok(snapshot)
    }

    fn checkpoint(&self) -> Result<(), DriverError> {
        if let Some(work) = &self.work {
            let mark = work.trees_mark();
            let trees = self.saved.load(Ordering::Relaxed) != mark;
            work.save(&log::work_path(self.log.path()), trees)?;
            self.saved.store(mark, Ordering::Relaxed);
        }
        Ok(())
    }

    fn append(&mut self, line: Line) -> Result<(), DriverError> {
        self.log.append(&line)?;
        self.pending.push(line.clone());
        self.state.apply(line);
        Ok(())
    }

    pub(crate) fn take_pending(&mut self) -> Vec<Line> {
        std::mem::take(&mut self.pending)
    }

    pub(crate) fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    fn take(&mut self) -> u64 {
        let value = self.next;
        self.next = self.next.saturating_add(1);
        value
    }
}

pub(crate) fn work_at(path: &Path, workspace: &Path, policy: Policy, text: &str) -> bool {
    let Ok(Line::Session {
        workspace: recorded,
        ..
    }) = log::header(path)
    else {
        return false;
    };
    if recorded != canonical(workspace) {
        return false;
    }
    Work::load(&log::work_path(path))
        .ok()
        .flatten()
        .is_some_and(|work| {
            std::mem::discriminant(&work.policy) == std::mem::discriminant(&policy)
                && work.objective == text
                && !work.terminal()
        })
}

pub(crate) fn touched_at(path: &Path) -> Vec<String> {
    Work::load(&log::work_path(path))
        .ok()
        .flatten()
        .map(|work| work.touched.into_iter().collect())
        .unwrap_or_default()
}

pub(crate) fn active_at(path: &Path, workspace: &Path) -> Option<(Policy, String)> {
    let Ok(Line::Session {
        workspace: recorded,
        ..
    }) = log::header(path)
    else {
        return None;
    };
    if recorded != canonical(workspace) {
        return None;
    }
    Work::load(&log::work_path(path))
        .ok()
        .flatten()
        .filter(|work| work.policy != Policy::Single && !work.terminal())
        .map(|work| (work.policy, work.objective))
}

pub(crate) fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
