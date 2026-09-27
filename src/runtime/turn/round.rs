use super::*;

impl<P: Provider> Loop<P> {
    pub(super) async fn run_turn(
        &mut self,
        mut messages: Vec<Message>,
        pre: &mut Pretask,
        mut emit: impl FnMut(&Event),
        events: &mut Vec<Event>,
        usage: &mut Usage,
        reached: &mut usize,
    ) -> Result<Turn> {
        let abort_reason;
        self.settle_mcp(events, &mut emit).await?;
        let tools = self.advertised_tools().to_vec();
        if std::mem::take(&mut self.accepted) {
            self.push_event(
                events,
                &mut emit,
                Event::Note {
                    text: "plan accepted — starting implementation".to_string(),
                },
            )?;
        }
        self.plan_first(&mut messages, usage, events, &mut emit)
            .await?;
        self.scout(&mut messages, usage, events, &mut emit).await?;

        loop {
            while self.budget.max_tokens.is_some() && self.child_budget_reserved() > 0 {
                self.await_children(events, &mut emit, usage).await?;
            }
            *reached = reached.saturating_add(1);
            let round = *reached;
            if let Some(reason) = self.guard.stuck()
                && let Some(reason) = self
                    .steer_stuck(reason, &mut messages, events, &mut emit)
                    .await?
            {
                abort_reason = Some(reason);
                break;
            }
            if self.checkpoint_rounds > 0 && round.is_multiple_of(self.checkpoint_rounds) {
                self.steer_checkpoint(round, &mut messages, events, &mut emit)
                    .await?;
            }
            if let Some(reason) = self.round_abort() {
                abort_reason = Some(reason);
                break;
            }
            let round_tools = tools.as_slice();
            if let Err(error) = self
                .prepare_round(&mut messages, round_tools, usage, events, &mut emit)
                .await
            {
                if let Some(reason) = self.round_abort() {
                    abort_reason = Some(reason);
                    break;
                }
                return Err(error);
            }
            if let Some(reason) = self.round_abort() {
                abort_reason = Some(reason);
                break;
            }
            let Some((assistant, prefix_hash, request_chars, durable_attempt, live_millis)) = self
                .complete_retry(&mut messages, round_tools, events, &mut emit)
                .await?
            else {
                abort_reason = self.round_abort();
                break;
            };
            if assistant.usage.prompt_tokens > 0 {
                self.context_sample = Some((request_chars, assistant.usage.prompt_tokens));
            }
            let role = self.round_role();
            let meta = self.record_usage(&assistant, prefix_hash, role, usage);
            let stop_reason = assistant.stop_reason;
            let meter = super::meter(&assistant.usage, live_millis);
            let reasoning = assistant.reasoning_content.clone().unwrap_or_default();
            let message = assistant.into_message();
            let calls = message.tool_calls.clone();
            messages.push(message.clone());
            if stop_reason == StopReason::Length && calls.is_empty() {
                self.steer_length(
                    &message,
                    &reasoning,
                    durable_attempt,
                    meter,
                    meta,
                    &mut messages,
                    events,
                    &mut emit,
                )
                .await?;
                continue;
            }
            if calls.is_empty() {
                return self
                    .finish_turn(
                        message,
                        events,
                        emit,
                        round,
                        usage,
                        durable_attempt,
                        reasoning,
                        meter,
                        meta,
                        pre,
                    )
                    .await;
            }
            let planned = self
                .session
                .finish_provider_with_tools(
                    durable_attempt,
                    &message.content,
                    &reasoning,
                    meter,
                    &calls,
                    meta,
                )
                .map_err(super::session_err)?;
            self.emit_records(events, &mut emit)?;
            self.settle(pre, events, &mut emit).await?;
            self.execute_calls(planned, calls, &mut messages, events, &mut emit, usage)
                .await?;
        }
        self.abort_turn(abort_reason, events, emit, usage, *reached, pre)
            .await
    }

    pub(super) async fn plan_first(
        &mut self,
        messages: &mut Vec<Message>,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if self.is_plan() || !self.plan_first || !self.session.needs_turn_plan() {
            return Ok(());
        }
        let objective = self.session.objective().map_err(super::session_err)?;
        let before = usage.clone();
        self.tools.set_read_only(true);
        self.schema = None;
        let drafted = plan::draft(self, &objective, usage).await;
        self.tools.set_read_only(self.is_plan());
        self.schema = None;
        self.seat_usage(workflow::Role::Plan, &before, usage, events, emit)?;
        let commands = self.verify.clone().unwrap_or_default();
        match drafted {
            Ok(text) => {
                self.session
                    .select_plan(&text, &commands)
                    .map_err(super::session_err)?;
                let input = UserInput::from(plan::note(&text));
                self.session.steer(&input).map_err(super::session_err)?;
                messages.push(Message::user(input));
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: "plan written before execution".to_string(),
                    },
                )?;
            }
            Err(error) => {
                self.session
                    .begin_without_plan(&commands)
                    .map_err(super::session_err)?;
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!(
                            "planning unavailable — executing without a written plan: {error}"
                        ),
                    },
                )?;
            }
        }
        let progress = self.work_progress()?;
        self.push_event(events, emit, Event::Work { progress })?;
        Ok(())
    }

    pub(super) async fn scout(
        &mut self,
        messages: &mut Vec<Message>,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if self.seat_model(workflow::Role::Scout).is_none()
            || !self.session.needs_scout(self.is_plan())
        {
            return Ok(());
        }
        let objective = self.session.objective().map_err(super::session_err)?;
        let before = usage.clone();
        let brief = scout::brief(self, &objective, usage).await;
        self.seat_usage(workflow::Role::Scout, &before, usage, events, emit)?;
        let brief = match brief {
            Ok(brief) => brief,
            Err(error) => {
                return self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("scout skipped — {error}"),
                    },
                );
            }
        };
        self.session.set_brief(&brief).map_err(super::session_err)?;
        let input = UserInput::from(scout::note(&brief));
        self.session.steer(&input).map_err(super::session_err)?;
        messages.push(Message::user(input));
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "scout brief added to the conversation".to_string(),
            },
        )
    }

    pub(super) fn begin_turn(&mut self, mut input: UserInput) -> Result<Vec<Message>> {
        if input.text.trim().is_empty() && input.images.is_empty() {
            return Err(Error::Config("the prompt is empty".to_string()));
        }
        let child_read_only = self.depth > 0 && self.tools.is_read_only();
        let read_only = self.is_plan() || child_read_only;
        if read_only != self.tools.is_read_only() {
            self.schema = None;
        }
        self.tools.set_read_only(read_only);
        if self.depth == 0 {
            self.tools.reset_edits();
            self.asked.retain(|grant| !matches!(grant, Grant::Edit(_)));
        }
        self.summary_failed = false;
        self.guard.reset();
        self.turn_started = crate::mail::now();
        let commands = self.verify.clone().unwrap_or_default();
        self.prepare_work(&input.text, &commands)?;
        let mut harness = self.harness_note(&self.work_progress()?);
        if self.accepted {
            harness.push_str(
                "\n\nHarness: the selected plan is now accepted; /plan reject discards it.",
            );
        }
        input.text = decorate(&harness, &input.text);
        self.session
            .start_input(&input)
            .map_err(super::session_err)?;
        let history = self.session.history();
        let mut messages = Vec::with_capacity(history.len() + 1);
        messages.push(Message::system(self.system_prompt.clone()));
        messages.extend(history.iter().cloned());
        Ok(messages)
    }

    fn prepare_work(&mut self, text: &str, commands: &[String]) -> Result<()> {
        if self.is_plan() {
            self.session
                .prepare_plan_work(text)
                .map_err(super::session_err)?;
            return Ok(());
        }
        if self.is_goal()
            && self
                .session
                .goal_objective()
                .map_err(super::session_err)?
                .is_none()
        {
            self.select_goal(text, commands)?;
        }
        if let TurnWork::Ready { accepted } = self
            .session
            .prepare_turn_work(text, commands, self.plan_first)
            .map_err(super::session_err)?
        {
            self.accepted = accepted;
        }
        Ok(())
    }

    async fn steer_stuck(
        &mut self,
        reason: String,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<String>> {
        let interactive = self.steer_shared.is_some();
        if interactive && self.guard.steered() {
            let question = format!("{reason}. Let the turn continue?");
            if self.approve(question, false, events, emit).await? == GrantAnswer::Deny {
                return Ok(Some(reason));
            }
            self.guard.forgive();
            return Ok(None);
        }
        let input = UserInput::from(format!(
            "[summon control: {reason}. Before the next tool call, state the \
             hypothesis the next action tests and what result would falsify it. \
             If the same outcome repeats, change the approach.]"
        ));
        self.session.steer(&input).map_err(super::session_err)?;
        messages.push(Message::user(input));
        self.guard.mark_steered();
        if !interactive {
            self.guard.forgive();
        }
        self.push_event(
            events,
            emit,
            Event::Note {
                text: format!("loop guard: {reason} — steering the model"),
            },
        )?;
        Ok(None)
    }

    async fn steer_checkpoint(
        &mut self,
        round: usize,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let progress = self.work_progress()?;
        let mut text = String::from(
            "[summon control: checkpoint — update the ledger before continuing: what is \
             proven, what is the current blocker, what is next. If the last actions did \
             not move the work forward, change approach or ask.]",
        );
        if !progress.ledger.is_empty() {
            text.push_str("\nCurrent ledger:\n");
            text.push_str(&progress.ledger.join("\n"));
        }
        let input = UserInput::from(text);
        self.session.steer(&input).map_err(super::session_err)?;
        messages.push(Message::user(input));
        self.push_event(
            events,
            emit,
            Event::Note {
                text: format!("checkpoint at round {round}"),
            },
        )
    }

    pub(in crate::runtime) fn round_abort(&self) -> Option<String> {
        self.budget.breach(self.spent_tokens, self.elapsed_secs())
    }

    pub(super) async fn prepare_round(
        &mut self,
        messages: &mut Vec<Message>,
        tools: &[ToolDefinition],
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        self.mail_heartbeat("working");
        self.settle_probe().await;
        self.take_mail(messages, events, emit)?;
        self.drain(events, emit);
        self.auto_compact(messages, tools, usage, events, emit)
            .await
    }
}
