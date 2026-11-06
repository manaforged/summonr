use super::*;

impl<P: Provider> Loop<P> {
    fn close_plain(&mut self, snapshot: String, events: &[Event]) -> Result<()> {
        if self.session.stage() == Some(Stage::Execute)
            && self.work_progress()?.kind == crate::WorkKind::Single
        {
            let wrote = self.tools.wrote();
            drop(self.session.enter_verify(&wrote)?);
            drop(self.session.limit_changes(&wrote)?);
            let receipt = self.turn_receipt(&wrote, snapshot, events);
            let verdict = self.session.captured_verdict(true);
            self.session
                .commit_verdict(verdict, Decision::Automatic, Some(&receipt))?;
        } else {
            self.session.close()?;
        }
        Ok(())
    }

    fn turn_receipt(&self, wrote: &[String], snapshot: String, events: &[Event]) -> Receipt {
        crate::verify::from_turn(
            wrote.to_vec(),
            snapshot,
            events,
            self.verify.as_deref().unwrap_or(&[]),
        )
    }

    pub(super) async fn finish_turn(
        &mut self,
        message: Message,
        events: &mut Vec<Event>,
        mut emit: impl FnMut(&Event),
        round: usize,
        usage: &mut Usage,
        pre: &mut Pretask,
    ) -> Result<Turn> {
        let snapshot = self.settle(pre, events, &mut emit).await?;
        if !self.executes_work() {
            self.close_plain(snapshot, events)?;
            return self.close_turn(
                Ending::Done(Vec::new()),
                message.content,
                events,
                emit,
                round,
                usage,
            );
        }
        let (checks, mut running, unsettled) = self.await_jobs(events, &mut emit).await?;
        if let Some(abort) = self
            .children_abort(&mut running, events, &mut emit, usage)
            .await?
        {
            return self
                .abort_turn(abort, events, emit, usage, round, pre)
                .await;
        }
        if let Some(running) = self.defer_running(&running, unsettled)? {
            let ending = Ending::Deferred(running);
            return self.close_turn(ending, message.content, events, emit, round, usage);
        }
        self.stop_spawned(events, &mut emit, usage).await?;
        let checks = self
            .judge_work(&message.content, snapshot, checks, events, &mut emit, usage)
            .await?;
        self.close_turn(
            Ending::Done(checks),
            message.content,
            events,
            emit,
            round,
            usage,
        )
    }

    async fn judge_work(
        &mut self,
        reply: &str,
        snapshot: String,
        mut checks: Vec<Outcome>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Vec<Outcome>> {
        let wrote = self.tools.wrote();
        drop(self.session.enter_verify(&wrote)?);
        let paths = self.session.limit_changes(&wrote)?;
        checks.extend(self.run_verify(events, emit).await?);
        if checks.is_empty() {
            checks.extend(crate::verify::ran(
                events,
                self.verify.as_deref().unwrap_or(&[]),
            ));
        }
        if let Some(measured) = self.measure_turn(&checks, events, emit).await? {
            checks.push(measured);
        }
        let mut verdict = self.verdict(&mut checks, events, emit)?;
        let receipt = crate::verify::new(paths, checks.clone(), snapshot);
        let decision = self
            .decide(
                reply,
                &receipt.text,
                &mut verdict,
                &mut checks,
                events,
                emit,
                usage,
            )
            .await?;
        let receipt = crate::verify::new(receipt.changes, checks.clone(), receipt.snapshot);
        self.session
            .commit_verdict(verdict, decision, Some(&receipt))?;
        Ok(checks)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the verdict's inputs move through as one call"
    )]
    async fn decide(
        &mut self,
        reply: &str,
        receipt: &str,
        verdict: &mut Verdict,
        checks: &mut Vec<Outcome>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Decision> {
        if !verdict.passed {
            return Ok(Decision::Automatic);
        }
        let decision = self
            .goal_decision(reply, receipt, events, emit, usage)
            .await?;
        if !matches!(decision, Decision::Automatic) {
            *verdict = self.verdict(checks, events, emit)?;
        }
        Ok(decision)
    }

    async fn goal_decision(
        &mut self,
        reply: &str,
        evidence: &str,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Decision> {
        let Some(objective) = self.session.goal_objective()? else {
            return Ok(Decision::Automatic);
        };
        if self.metric.is_some() {
            return Ok(Decision::GoalComplete);
        }
        if let Some(decision) = self.adopt_contract(&objective, events, emit, usage).await? {
            return Ok(decision);
        }
        let todos = self.session.work_progress()?.todos;
        let open = todos
            .iter()
            .filter(|todo| todo.open() && crate::limits::limits().stall_rounds > 0)
            .map(|todo| todo.text.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        if open.is_empty()
            && self
                .session
                .completion_checks()?
                .iter()
                .any(|check| check.observation.is_none())
        {
            return Ok(Decision::GoalComplete);
        }
        let evidence = match open.as_str() {
            "" => evidence.to_string(),
            open => format!("{evidence}\nOpen tasks: {open}"),
        };
        let evidence = goal::with_corrections(&evidence, events);
        let decision = goal::decision(self, &objective, reply, &evidence, usage).await;
        self.sync_usage(emit);
        Ok(decision)
    }

    fn close_turn(
        &mut self,
        ending: Ending,
        text: String,
        events: &mut Vec<Event>,
        mut emit: impl FnMut(&Event),
        round: usize,
        usage: &mut Usage,
    ) -> Result<Turn> {
        self.watch_jobs();
        let progress = self.work_progress()?;
        self.push_event(events, &mut emit, Event::Work { progress })?;
        let (note, checks) = match ending {
            Ending::Deferred(running) => (
                Some(format!(
                    "turn deferred; background work still running: {running}"
                )),
                Vec::new(),
            ),
            Ending::Done(checks) => {
                self.emit_records(events, &mut emit)?;
                (None, checks)
            }
        };
        if let Some(text) = note {
            self.push_event(events, &mut emit, Event::Note { text })?;
        }
        self.push_event(events, &mut emit, Event::Assistant { text: text.clone() })?;
        Ok(Turn {
            text,
            events: std::mem::take(events),
            rounds: round,
            usage: std::mem::take(usage),
            checks,
            aborted: false,
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one close path for a length-stopped provider round"
    )]
    pub(super) async fn steer_length(
        &mut self,
        message: &Message,
        reasoning: &str,
        durable_attempt: Attempt,
        meter: Meter,
        meta: UsageMeta,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        self.session
            .finish_provider(durable_attempt, &message.content, reasoning, meter, meta)?;
        self.emit_records(events, emit)?;
        self.inject(
            "[summon control: the previous reply reached the output limit and was cut off.]"
                .to_string(),
            messages,
        )?;
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "round output limit reached; continuing with the same context".to_string(),
            },
        )
    }
}

enum Ending {
    Deferred(String),
    Done(Vec<Outcome>),
}
