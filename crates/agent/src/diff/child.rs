use std::collections::HashSet;
use std::path::Path;

use super::turn::Span;
use super::{Diff, Stat};
use crate::checkpoint::Checkpoints;
use crate::child::{ChildRecord, ChildState};
use crate::durable::log::{self, State};
use summonr_core::error::{Error, Result};

/// A child's change to the lead checkout: its own files, and files that moved in the same span without its tools.
pub struct Delta {
    pub diff: Diff,
    points: Checkpoints,
    span: Span,
}

impl Delta {
    /// The patch of the child's own files, or of one of them.
    pub fn patch(&self, path: Option<&str>) -> Result<String> {
        self.span.patch(&self.points, &self.diff.files, path)
    }
}

/// The children a session started, in journal order.
pub fn children(journal: &Path) -> Result<Vec<ChildRecord>> {
    let mut state = State::default();
    log::records(journal, 0)
        .try_for_each(|line| line.map(|line| state.apply(line)))
        .map_err(|error| Error::Config(format!("cannot read the session log: {error}")))?;
    Ok(state.children.into_values().collect())
}

/// The delta of `record` in the lead checkout at `root`.
pub fn delta(root: &Path, record: &ChildRecord) -> Result<Delta> {
    let id = record.id.to_string();
    if record.base.is_empty() {
        return Err(Error::Config(format!(
            "child {id} has no start snapshot; it ran read-only or before deltas were recorded"
        )));
    }
    let points = Checkpoints::open(root, &id)
        .ok_or_else(|| Error::Config("this workspace has no checkpoints".to_string()))?;
    let result = record.state.result();
    let (to, state) = match result {
        Some(result) if !result.head.is_empty() => (result.head.clone(), "done"),
        Some(_) if matches!(record.state, ChildState::Completed(_)) => {
            return Err(Error::Config(format!(
                "child {id} finished without landing in this checkout; its changes were kept at {}",
                record.workspace.display()
            )));
        }
        Some(_) => (points.tree()?, "stopped"),
        None => (points.tree()?, "running"),
    };
    let span = Span {
        base: format!("child {} ({state})", crate::child::tag(&id)),
        from: record.base.clone(),
        to,
    };
    let mut diff = span.stats(&points, &id)?;
    let own: HashSet<&str> = result
        .map(|result| result.changed_paths.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let (files, strays): (Vec<Stat>, Vec<Stat>) = std::mem::take(&mut diff.files)
        .into_iter()
        .partition(|stat| own.contains(stat.path.as_str()));
    diff.files = files;
    diff.strays = strays;
    Ok(Delta { diff, points, span })
}
