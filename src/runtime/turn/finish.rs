use super::*;

impl<P: Provider> Loop<P> {
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
            self.session
                .fail_provider(
                    attempt,
                    "assistant returned neither text nor a tool call",
                    meter.millis,
                    true,
                )
                .map_err(super::session_err)?;
            self.emit_records(events, &mut emit)?;
            return Err(Error::provider(
                ProviderFailureKind::Malformed,
                "assistant returned neither text nor a tool call",
            ));
        }
        if self.is_plan() {
            let receipt = Receipt::new(Vec::new(), Vec::new(), snapshot, events);
            self.session
                .finish_plan(
                    attempt,
                    &message.content,
                    &reasoning,
                    meter,
                    meta,
                    self.verify.as_deref().unwrap_or_default(),
                    &receipt,
                )
                .map_err(super::session_err)?;
            self.emit_records(events, &mut emit)?;
            let progress = self.work_progress()?;
            self.push_event(events, &mut emit, Event::Work { progress })?;
            self.push_event(
                events,
                &mut emit,
                Event::Note {
                    text: format!(
                        "plan saved to {}",
                        crate::plan::file(&self.workspace).display()
                    ),
                },
            )?;
            let text = message.content;
            self.push_event(events, &mut emit, Event::Assistant { text: text.clone() })?;
            return Ok(Turn {
                text,
                events: std::mem::take(events),
                rounds: round,
                usage: std::mem::take(usage),
                checks: Vec::new(),
                aborted: false,
            });
        }
        self.session
            .finish_provider(attempt, &message.content, &reasoning, meter, meta)
            .map_err(super::session_err)?;
        self.emit_records(events, &mut emit)?;
        let (text, checks) = if self.session.stage() == Some(Stage::Execute)
            && (self.is_work() || self.is_goal())
        {
            let (mut checks, mut running, unsettled) = self.await_jobs(events, &mut emit).await?;
            let before_children = self.spent_tokens;
            running.extend(self.await_children(events, &mut emit, usage).await?);
            if self.spent_tokens > before_children
                && let Some(reason) = self.round_abort()
            {
                return self
                    .abort_turn(Some(reason), events, emit, usage, round, pre)
                    .await;
            }
            if !running.is_empty() {
                let note = format!(
                    "[summon: background work still running — {} — wait for it with the job or child id, or kill it, before you report done]",
                    running.join("; ")
                );
                if self
                    .session
                    .defer(&note, unsettled)
                    .map_err(super::session_err)?
                {
                    let progress = self.work_progress()?;
                    self.push_event(events, &mut emit, Event::Work { progress })?;
                    self.push_event(
                        events,
                        &mut emit,
                        Event::Note {
                            text: format!(
                                "turn deferred — background work still running: {}",
                                running.join("; ")
                            ),
                        },
                    )?;
                    let text = message.content;
                    self.push_event(events, &mut emit, Event::Assistant { text: text.clone() })?;
                    return Ok(Turn {
                        text,
                        events: std::mem::take(events),
                        rounds: round,
                        usage: std::mem::take(usage),
                        checks: Vec::new(),
                        aborted: false,
                    });
                }
            }
            let killed = self.kill_spawned("the parent turn ended", usage).await?;
            if !killed.is_empty() {
                self.push_event(
                    events,
                    &mut emit,
                    Event::Note {
                        text: format!(
                            "stopped {} background child(ren) the turn left running",
                            killed.len()
                        ),
                    },
                )?;
            }
            let paths = self.session.enter_verify().map_err(super::session_err)?;
            checks.extend(self.run_verify(events, &mut emit).await?);
            let mut verdict = self.verdict(&mut checks, events, &mut emit)?;
            let mut rejected = None;
            if verdict.passed && self.seat_model(workflow::Role::Review).is_some() {
                self.session.enter_review().map_err(super::session_err)?;
                let objective = self.session.objective().map_err(super::session_err)?;
                let evidence = verify::summary(&paths, &checks);
                let before = usage.clone();
                let reviewed =
                    review::outcome(self, &objective, &message.content, &evidence, &paths, usage)
                        .await;
                self.seat_usage(workflow::Role::Review, &before, usage, events, &mut emit)?;
                match reviewed {
                    Ok(review) => {
                        self.push_event(
                            events,
                            &mut emit,
                            Event::Verify {
                                command: review.command.clone(),
                                passed: review.passed,
                            },
                        )?;
                        if !review.passed {
                            verdict.passed = false;
                            rejected = Some(review.output.clone());
                            self.push_event(
                                events,
                                &mut emit,
                                Event::Note {
                                    text: format!("review findings:\n{}", review.output),
                                },
                            )?;
                        }
                        checks.push(review);
                    }
                    Err(why) => {
                        let review = Outcome {
                            command: review::CHECK.to_string(),
                            passed: false,
                            output: why,
                        };
                        self.push_event(
                            events,
                            &mut emit,
                            Event::Verify {
                                command: review.command.clone(),
                                passed: false,
                            },
                        )?;
                        let why = review.output.clone();
                        verdict.passed = false;
                        rejected = Some(why.clone());
                        checks.push(review);
                        self.push_event(
                            events,
                            &mut emit,
                            Event::Note {
                                text: format!("review unavailable — {why}"),
                            },
                        )?;
                    }
                }
            }
            let receipt = Receipt::new(paths, checks.clone(), snapshot, events);
            let decision = if verdict.passed {
                if let Some(objective) =
                    self.session.goal_objective().map_err(super::session_err)?
                {
                    let before = usage.clone();
                    let decision =
                        goal::decision(self, &objective, &message.content, &receipt.text, usage)
                            .await;
                    self.seat_usage(workflow::Role::Judge, &before, usage, events, &mut emit)?;
                    decision
                } else {
                    Decision::Automatic
                }
            } else {
                Decision::Automatic
            };
            if verdict.passed {
                verdict = self.verdict(&mut checks, events, &mut emit)?;
            }
            let receipt = Receipt::new(receipt.changes, checks.clone(), receipt.snapshot, events);
            match rejected {
                Some(findings) => self
                    .session
                    .commit_review(&findings, Some(&receipt))
                    .map_err(super::session_err)?,
                None => self
                    .session
                    .commit_verdict(verdict, decision, Some(&receipt))
                    .map_err(super::session_err)?,
            }
            (message.content, checks)
        } else {
            self.session.close().map_err(super::session_err)?;
            (message.content, Vec::new())
        };
        let progress = self.work_progress()?;
        self.push_event(events, &mut emit, Event::Work { progress })?;
        self.emit_records(events, &mut emit)?;
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
        let control = if message.content.trim().is_empty() && !reasoning.is_empty() {
            UserInput::from(
                "/no_think [summon control: the previous round used its output on \
                 reasoning. Stop reasoning and give the concise final answer now.]",
            )
        } else {
            UserInput::from(
                "[summon control: the previous round reached its output limit. Continue \
                 briefly from the partial answer.]",
            )
        };
        self.session.steer(&control).map_err(super::session_err)?;
        messages.push(Message::user(control));
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "round output limit reached — continuing with the same context".to_string(),
            },
        )
    }
}
