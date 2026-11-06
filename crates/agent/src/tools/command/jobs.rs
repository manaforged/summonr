use std::path::Path;

use super::proc;
use super::proc::stop_job;
use crate::tools::ToolRegistry;
use summonr_core::error::{Error, Result};

impl ToolRegistry {
    pub(crate) fn stop_jobs(&self) -> Result<Vec<String>> {
        self.stop_matching(|_| true)
    }

    pub(crate) fn stop_detached(&self) -> Result<Vec<String>> {
        let ids = std::mem::take(
            &mut *self
                .detached_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        self.stop_matching(|record| ids.contains(&record.id))
    }

    pub(crate) fn clear_detached(&self) {
        self.detached_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    pub(crate) fn see_job(&self, id: &str) {
        self.seen_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.to_string());
    }

    pub(crate) fn job_seen(&self, id: &str) -> bool {
        self.seen_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(id)
    }

    fn stop_matching(&self, stops: impl Fn(&crate::jobs::Record) -> bool) -> Result<Vec<String>> {
        let session = self
            .session
            .lock()
            .map_err(|error| Error::Tool(format!("cannot read tool session: {error}")))?;
        let Some(session) = session.clone() else {
            return Ok(Vec::new());
        };
        stop_matching(&self.workspace, &session, stops)
    }
}

pub(crate) fn kill_job(workspace: &Path, id: &str) -> Result<crate::jobs::Status> {
    let record = crate::jobs::load(workspace, id)?;
    stop_job(workspace, &record)?;
    crate::jobs::status(workspace, &record)
}

/// Stops background jobs owned by the given session.
pub(crate) fn stop_jobs(workspace: &Path, session: &str) -> Result<Vec<String>> {
    stop_matching(workspace, session, |_| true)
}

static CHILDREN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Records a child session whose background jobs stop when this process exits.
pub(crate) fn own_child(session: &str) {
    CHILDREN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(session.to_string());
}

/// Stops the background jobs of every child session this process started: every group first, then the records.
pub(crate) fn stop_child_jobs() {
    let children = std::mem::take(
        &mut *CHILDREN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    let jobs: Vec<crate::jobs::Record> = crate::jobs::started_here()
        .into_iter()
        .filter(|record| {
            record
                .session
                .as_ref()
                .is_some_and(|owner| children.contains(owner))
        })
        .collect();
    for record in &jobs {
        if crate::jobs::owner(record.pid, record.started) == crate::jobs::Owner::Job {
            proc::kill_group(record.pid);
        }
    }
    for record in &jobs {
        drop(stop_job(Path::new(&record.workspace), record));
    }
}

fn stop_matching(
    workspace: &Path,
    session: &str,
    stops: impl Fn(&crate::jobs::Record) -> bool,
) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    for record in crate::jobs::since(workspace, 0, Some(session))
        .into_iter()
        .filter(|record| record.session.as_deref() == Some(session) && stops(record))
    {
        if crate::jobs::status(workspace, &record)?.state == crate::jobs::JobState::Running {
            let children = stop_job(workspace, &record)?;
            notes.push(format!(
                "killed job {} `{}`\n{}",
                record.id,
                crate::secret::shield::scrub_found(&record.command, workspace),
                crate::jobs::block("child commands at kill", &children)
            ));
        }
    }
    Ok(notes)
}
