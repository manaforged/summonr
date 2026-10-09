use super::close::Answer;
use super::*;

impl<P: Provider> Loop<P> {
    fn close_plain(&mut self, snapshot: String, events: &[Event]) -> Result<()> {
        if self.session.stage() == Some(Stage::Execute)
            && self.work_progress()?.kind == crate::WorkKind::Single
        {
            let paths = self.session.enter_verify().map_err(super::session_err)?;
            let receipt = Receipt::new(paths, Vec::new(), snapshot, events);
            let verdict = self
                .session
                .prepare_verdict(true)
                .map_err(super::session_err)?;
            self.session
                .commit_verdict(verdict, Decision::Automatic, Some(&receipt))
                .map_err(super::session_err)?;
        } else {
            self.session.close().map_err(super::session_err)?;
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one close path for a finished provider round"
    )]
    pub(super) async fn finish_turn(
        &mut self,
        message: Message,
        events: &mut Vec<Event>,
        mut emit: impl FnMut(&Event),
        round: usize,
        usage: &mut Usage,
        attempt: Attempt,
        reasoning: String,
        meter: Meter,
        meta: UsageMeta,
        pre: &mut Pretask,
    ) -> Result<Turn> {
        let snapshot = self.settle(pre, events, &mut emit).await?;
        if message.content.trim().is_empty() {
            return self.reject_empty(attempt, meter.millis, events, &mut emit);
        }
        let answer = Answer {
            attempt,
            text: &message.content,
            reasoning: &reasoning,
            meter,
            meta,
        };
        self.record_answer(answer, events, &mut emit)?;
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
        let paths = self.session.enter_verify().map_err(super::session_err)?;
        checks.extend(self.run_verify(events, emit).await?);
        let mut verdict = self.verdict(&mut checks, events, emit)?;
        let mut rejected = None;
        let mut reviewed = false;
        if verdict.passed && self.seat_model(workflow::Role::Review).is_some() {
            rejected = self
                .review_turn(reply, &paths, &mut checks, events, emit, usage)
                .await?;
            verdict.passed = rejected.is_none();
            reviewed = true;
        }
        let receipt = Receipt::new(paths, checks.clone(), snapshot, events);
        let decision = self
            .decide(
                reply,
                &receipt.text,
                &mut verdict,
                reviewed,
                &mut checks,
                events,
                emit,
                usage,
            )
            .await?;
        let receipt = Receipt::new(receipt.changes, checks.clone(), receipt.snapshot, events);
        match rejected {
            Some(findings) => self.session.commit_review(&findings, Some(&receipt)),
            None => self
                .session
                .commit_verdict(verdict, decision, Some(&receipt)),
        }
        .map_err(super::session_err)?;
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
        reviewed: bool,
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
        if reviewed || !matches!(decision, Decision::Automatic) {
            *verdict = self.verdict(checks, events, emit)?;
        }
        Ok(decision)
    }

    async fn review_turn(
        &mut self,
        reply: &str,
        paths: &[String],
        checks: &mut Vec<Outcome>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Option<String>> {
        self.session.enter_review().map_err(super::session_err)?;
        let objective = self.session.objective().map_err(super::session_err)?;
        let evidence = verify::summary(paths, checks);
        let before = usage.clone();
        let reviewed = review::outcome(self, &objective, reply, &evidence, paths, usage).await;
        self.seat_usage(workflow::Role::Review, &before, usage, events, emit)?;
        let (review, heading) = match reviewed {
            Ok(review) => (review, "review findings:\n"),
            Err(why) => (
                Outcome {
                    command: review::CHECK.to_string(),
                    passed: false,
                    output: why,
                },
                "",
            ),
        };
        self.push_event(
            events,
            emit,
            Event::Verify {
                command: review.command.clone(),
                passed: review.passed,
            },
        )?;
        let rejected = (!review.passed).then(|| review.output.clone());
        if let Some(output) = &rejected {
            let text = format!("{heading}{output}");
            self.push_event(events, emit, Event::Note { text })?;
        }
        checks.push(review);
        Ok(rejected)
    }

    async fn goal_decision(
        &mut self,
        reply: &str,
        evidence: &str,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Decision> {
        let Some(objective) = self.session.goal_objective().map_err(super::session_err)? else {
            return Ok(Decision::Automatic);
        };
        let before = usage.clone();
        let decision = goal::decision(self, &objective, reply, evidence, usage).await;
        self.seat_usage(workflow::Role::Judge, &before, usage, events, emit)?;
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
            .finish_provider(durable_attempt, &message.content, reasoning, meter, meta)
            .map_err(super::session_err)?;
        self.emit_records(events, emit)?;
        let control = UserInput::from(
            "[summon control: the previous reply reached the output limit and was cut off.]",
        );
        self.session.steer(&control).map_err(super::session_err)?;
        messages.push(Message::user(control));
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
