use super::*;

pub(super) struct Answer<'a> {
    pub(super) attempt: Attempt,
    pub(super) text: &'a str,
    pub(super) reasoning: &'a str,
    pub(super) meter: Meter,
    pub(super) meta: UsageMeta,
}

impl<P: Provider> Loop<P> {
    pub(super) fn reject_empty<T>(
        &mut self,
        attempt: Attempt,
        millis: u64,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<T> {
        self.session.fail_provider(
            attempt,
            "assistant returned neither text nor a tool call",
            millis,
            true,
        )?;
        self.emit_records(events, emit)?;
        Err(Error::provider(
            ProviderFailureKind::Malformed,
            "assistant returned neither text nor a tool call",
        ))
    }

    pub(super) fn record_answer(
        &mut self,
        answer: Answer<'_>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        self.session.finish_provider(
            answer.attempt,
            answer.text,
            answer.reasoning,
            answer.meter,
            answer.meta,
        )?;
        self.emit_records(events, emit)
    }

    pub(super) async fn children_abort(
        &mut self,
        running: &mut Vec<String>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Option<Abort>> {
        let before_children = self.spent_tokens();
        running.extend(self.await_children(events, emit, usage).await?);
        if self.spent_tokens() > before_children {
            return Ok(self.budget_abort());
        }
        Ok(None)
    }

    pub(super) fn close_open(&mut self, reason: &str, emit: &mut impl FnMut(&Event)) {
        if !self.session.turn_open() {
            return;
        }
        let abort = Abort {
            code: crate::tools::AbortCode::Error,
            reason: reason.to_string(),
        };
        if let Err(closing) = self.abort_with(&abort, &mut *emit) {
            emit(&Event::Note {
                text: format!("{reason}: closing the turn reported an error: {closing}"),
            });
        }
    }

    pub(super) fn executes_work(&self) -> bool {
        self.mode != WorkKind::Single
            && self.session.stage() == Some(Stage::Execute)
            && self
                .work_progress()
                .is_ok_and(|progress| progress.kind == self.mode)
    }

    fn owned_jobs(&self) -> Vec<jobs::Record> {
        let session = self.session.id().to_string();
        jobs::since(&self.workspace, 0, Some(&session))
            .into_iter()
            .filter(|record| record.session.as_deref() == Some(session.as_str()))
            .collect()
    }

    pub(crate) fn watch_jobs(&mut self) {
        for record in self.owned_jobs() {
            if self.job_known.insert(record.id.clone()) {
                self.job_watch.push(record);
            }
        }
    }

    pub(crate) fn rewatch_jobs(&mut self, last_write: Option<std::time::SystemTime>) {
        self.job_watch.clear();
        self.job_known.clear();
        for record in self.owned_jobs() {
            self.job_known.insert(record.id.clone());
            let exited_since = std::fs::metadata(jobs::exit_path(&self.workspace, &record.id))
                .and_then(|meta| meta.modified())
                .ok()
                .zip(last_write)
                .is_some_and(|(exited, written)| exited >= written);
            if exited_since || !jobs::finished(&self.workspace, &record) {
                self.job_watch.push(record);
            }
        }
    }

    pub(crate) fn finished_jobs(&mut self) -> Vec<String> {
        let (done, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.job_watch)
            .into_iter()
            .partition(|record| jobs::finished(&self.workspace, record));
        self.job_watch = waiting;
        done.iter()
            .filter_map(|record| self.job_exit_note(record))
            .collect()
    }

    fn job_exit_note(&self, record: &jobs::Record) -> Option<String> {
        if self.tools.job_seen(&record.id) {
            return None;
        }
        let state = match jobs::status(&self.workspace, record) {
            Ok(status) if status.state == jobs::JobState::Killed => return None,
            Ok(status) => status.label(),
            Err(error) => error.to_string(),
        };
        self.reported_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(record.id.clone());
        Some(format!(
            "background job {} finished ({state}): {}\nlog: {}",
            record.id,
            record.command,
            jobs::log_path(&self.workspace, record).display()
        ))
    }

    pub(super) fn job_reported(&self, id: &str) -> bool {
        self.reported_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(id)
    }

    pub(super) fn defer_running(
        &mut self,
        running: &[String],
        unsettled: Vec<String>,
    ) -> Result<Option<String>> {
        if running.is_empty() {
            return Ok(None);
        }
        let running = running.join("; ");
        let note = format!("[summon: background work still running: {running}]");
        let deferred = self.session.defer(&note, unsettled)?;
        Ok(deferred.then_some(running))
    }

    pub(super) async fn stop_spawned(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<()> {
        let killed = self.kill_spawned(crate::runtime::child::Kill::TurnEnded, usage).await?;
        if killed.is_empty() {
            return Ok(());
        }
        self.push_event(
            events,
            emit,
            Event::Note {
                text: format!("stopped {} subagent(s) the turn left running", killed.len()),
            },
        )
    }
}
