use super::*;

pub(super) struct Answer<'a> {
    pub(super) attempt: Attempt,
    pub(super) text: &'a str,
    pub(super) reasoning: &'a str,
    pub(super) meter: Meter,
    pub(super) meta: UsageMeta,
}

impl<P: Provider> Loop<P> {
    pub(super) fn reject_empty(
        &mut self,
        attempt: Attempt,
        millis: u64,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Turn> {
        self.session
            .fail_provider(
                attempt,
                "assistant returned neither text nor a tool call",
                millis,
                true,
            )
            .map_err(super::session_err)?;
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
        self.session
            .finish_provider(
                answer.attempt,
                answer.text,
                answer.reasoning,
                answer.meter,
                answer.meta,
            )
            .map_err(super::session_err)?;
        self.emit_records(events, emit)
    }

    pub(super) async fn children_abort(
        &mut self,
        running: &mut Vec<String>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Option<Abort>> {
        let before_children = self.spent_tokens;
        running.extend(self.await_children(events, emit, usage).await?);
        if self.spent_tokens > before_children {
            return Ok(self.budget_abort());
        }
        Ok(None)
    }

    pub(super) fn executes_work(&self) -> bool {
        self.session.stage() == Some(Stage::Execute) && (self.is_work() || self.is_goal())
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
        let deferred = self
            .session
            .defer(&note, unsettled)
            .map_err(super::session_err)?;
        Ok(deferred.then_some(running))
    }

    pub(super) async fn stop_spawned(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<()> {
        let killed = self.kill_spawned("the parent turn ended", usage).await?;
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
