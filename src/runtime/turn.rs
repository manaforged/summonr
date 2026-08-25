use super::compaction::force_compact;
use super::*;
use crate::ProviderFailureKind;
use crate::durable::context::AbortReason;

impl<P: Provider> Loop<P> {
    /// Inject user steering and freshly arrived bus mail into the
    /// conversation. Steering is the user's own words, so it lands
    /// verbatim; mail is peer text, never user instructions — the
    /// envelope says so, and each round injects at most a capped batch
    /// so a flooded mailbox cannot blow up the context.
    fn take_mail(
        &mut self,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let mut injected = false;
        while let Some(input) = self.steer.pop_front() {
            self.session
                .steer(&input.text, crate::durable::context::InputOrigin::User)
                .map_err(super::session_err)?;
            messages.push(Message::user(input));
            injected = true;
        }
        if let Some(shared) = &self.steer_shared
            && let Ok(mut queue) = shared.lock()
        {
            while let Some(input) = queue.pop_front() {
                self.session
                    .steer(&input.text, crate::durable::context::InputOrigin::User)
                    .map_err(super::session_err)?;
                messages.push(Message::user(input));
                injected = true;
            }
        }
        let (mail, overflow) = self.take_bus_mail(5)?;
        for mail in mail {
            let text = crate::bus::envelope(&mail.from, &mail.text);
            self.session
                .steer(&text, crate::durable::context::InputOrigin::Peer)
                .map_err(super::session_err)?;
            messages.push(Message::user(text.clone()));
            injected = true;
            self.push_event(
                events,
                emit,
                Event::Mail {
                    from: mail.from,
                    text: mail.text,
                },
            )?;
        }
        if overflow > 0 {
            let note = format!(
                "[summon bus: {overflow} more message(s) waiting — use the inbox tool to drain them]"
            );
            let input = UserInput::from(note);
            self.session
                .steer(&input.text, crate::durable::context::InputOrigin::Peer)
                .map_err(super::session_err)?;
            messages.push(Message::user(input));
            injected = true;
        }
        if injected {
            self.commit(messages);
        }
        Ok(())
    }

    /// Complete one round with retries: backoff on transient failures,
    /// and one compact-and-retry on context overflow.
    async fn complete_retry(
        &mut self,
        messages: &mut [Message],
        tools: &[crate::provider::ToolDefinition],
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<
        Option<(
            AssistantMessage,
            String,
            usize,
            crate::durable::id::ProviderAttemptId,
            u64,
        )>,
    > {
        let mut attempt = 0_u32;
        let mut overflowed = false;
        let mut wait = 2_u64;
        loop {
            let request = self.completion_request(messages, tools);
            let prefix_hash = request.prefix_hash();
            let request_chars = request_chars(messages, tools);
            let durable = request.durable_bytes().map_err(|error| {
                Error::Config(format!("cannot encode provider request: {error}"))
            })?;
            let durable_attempt = self
                .session
                .start_provider(&durable, super::exposure(&request, &durable))
                .map_err(super::session_err)?;
            let started = Instant::now();
            match self.provider.complete(&request).await {
                Ok(message) => {
                    return Ok(Some((
                        message,
                        prefix_hash,
                        request_chars,
                        durable_attempt,
                        super::elapsed_millis(started),
                    )));
                }
                Err(error) => {
                    self.session
                        .fail_provider(
                            durable_attempt,
                            super::durable_failure(error.provider_failure()),
                            &error.to_string(),
                            super::elapsed_millis(started),
                        )
                        .map_err(super::session_err)?;
                    self.spent_tokens = self.session.replay().spent_tokens;
                    if self.round_abort().is_some() {
                        return Ok(None);
                    }
                    let text = error.to_string();
                    let failure = error.provider_failure();
                    if !overflowed
                        && failure.is_some_and(|failure| {
                            failure.kind == ProviderFailureKind::ContextOverflow
                        })
                    {
                        if !force_compact(messages) {
                            return Err(error);
                        }
                        overflowed = true;
                        self.push_event(
                            events,
                            emit,
                            Event::Note {
                                text: "context overflow — compacted old tool results, retrying"
                                    .to_string(),
                            },
                        )?;
                        continue;
                    }
                    let retryable = failure.is_some_and(|failure| {
                        !failure.partial_output
                            && matches!(
                                failure.kind,
                                ProviderFailureKind::RateLimited
                                    | ProviderFailureKind::Server
                                    | ProviderFailureKind::Transport
                            )
                    });
                    if attempt >= 3 || !retryable {
                        return Err(error);
                    }
                    attempt += 1;
                    self.push_event(
                        events,
                        emit,
                        Event::Note {
                            text: format!(
                                "provider error — retry {attempt}/3 in {wait}s: {}",
                                text.lines().next().unwrap_or("unknown")
                            ),
                        },
                    )?;
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    wait *= 2;
                }
            }
        }
    }

    fn completion_request(
        &self,
        messages: &[Message],
        tools: &[crate::provider::ToolDefinition],
    ) -> CompletionRequest {
        CompletionRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            tools: tools.to_vec(),
            max_output_tokens: crate::runtime::DEFAULT_ROUND_OUTPUT_TOKENS,
            emit_deltas: true,
        }
    }

    pub(super) fn estimated_prompt_tokens(
        &self,
        messages: &[Message],
        tools: &[crate::provider::ToolDefinition],
    ) -> usize {
        let chars = request_chars(messages, tools);
        super::compaction::estimate(chars, self.context_sample)
    }

    /// Run one user turn, including any provider-requested tool calls.
    pub async fn turn(&mut self, prompt: &str) -> Result<Turn> {
        self.turn_with(prompt, |_| {}).await
    }

    /// Run one user turn and emit each event as it happens.
    pub async fn turn_with(
        &mut self,
        input: impl Into<UserInput>,
        emit: impl FnMut(&Event),
    ) -> Result<Turn> {
        let messages = self.begin_turn(input.into())?;
        self.run_turn(messages, emit).await
    }

    /// Run a turn from peer mail already observed by the interface.
    pub async fn turn_mail_with(&mut self, emit: impl FnMut(&Event)) -> Result<Turn> {
        if self.mail.is_empty() {
            return Err(Error::Config("there is no pending peer mail".to_string()));
        }
        self.turn_with(
            "[summon control: respond to the pending peer mail in this Work]",
            emit,
        )
        .await
    }

    async fn run_turn(
        &mut self,
        mut messages: Vec<Message>,
        mut emit: impl FnMut(&Event),
    ) -> Result<Turn> {
        let mut events = Vec::new();
        let mut usage = Usage::default();
        let mut abort_reason = None;
        let tools = self.advertised_tools().to_vec();

        for round in 1..=self.max_rounds {
            if let Some(reason) = self.round_abort() {
                abort_reason = Some(reason);
                break;
            }
            self.prepare_round(&mut messages, &tools, &mut usage, &mut events, &mut emit)
                .await?;
            if let Some(reason) = self.round_abort() {
                abort_reason = Some(reason);
                break;
            }
            let Some((assistant, prefix_hash, request_chars, durable_attempt, live_millis)) = self
                .complete_retry(&mut messages, &tools, &mut events, &mut emit)
                .await?
            else {
                abort_reason = self.round_abort();
                break;
            };
            if assistant.usage.prompt_tokens > 0 {
                self.context_sample = Some((request_chars, assistant.usage.prompt_tokens));
            }
            self.record_usage(&assistant, prefix_hash, &mut usage, &mut events, &mut emit)?;
            let stop_reason = assistant.stop_reason;
            let mut meter = super::meter(&assistant.usage);
            meter.live_millis = Some(live_millis);
            let reasoning = assistant.reasoning_content.clone().unwrap_or_default();
            let message = assistant.into_message();
            let calls = message.tool_calls.clone();
            messages.push(message.clone());
            if stop_reason == crate::provider::StopReason::Length && calls.is_empty() {
                self.session
                    .finish_provider(durable_attempt, &message.content, &reasoning, meter)
                    .map_err(super::session_err)?;
                let control = UserInput::from(
                    "[summon control: the previous round reached its output limit. Continue from \
                     that reasoning, keep the rest brief, and take the next concrete action.]",
                );
                self.session
                    .steer(
                        &control.text,
                        crate::durable::context::InputOrigin::HarnessControl,
                    )
                    .map_err(super::session_err)?;
                messages.push(Message::user(control));
                self.push_event(
                    &mut events,
                    &mut emit,
                    Event::Note {
                        text: "round output limit reached — continuing with the same context"
                            .to_string(),
                    },
                )?;
                self.commit(&messages);
                continue;
            }
            if calls.is_empty() {
                return self
                    .finish_turn(
                        message,
                        messages,
                        events,
                        emit,
                        round,
                        usage,
                        durable_attempt,
                        reasoning,
                        meter,
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
                    &calls
                        .iter()
                        .map(|call| {
                            (
                                call.function.name.as_str(),
                                call.function.arguments.as_str(),
                                call.id.as_str(),
                            )
                        })
                        .collect::<Vec<_>>(),
                )
                .map_err(super::session_err)?;
            self.execute_calls(planned, calls, &mut messages, &mut events, &mut emit)
                .await?;
        }
        self.abort_turn(abort_reason, events, emit, usage)
    }

    fn begin_turn(&mut self, mut input: UserInput) -> Result<Vec<Message>> {
        if input.text.trim().is_empty() && input.images.is_empty() {
            return Err(Error::Config("the prompt is empty".to_string()));
        }
        let commands = self.verify.as_deref().unwrap_or_default();
        if self.plan {
            self.session
                .prepare_plan_work(&input.text, commands)
                .map_err(super::session_err)?;
        } else {
            self.session
                .prepare_turn_work(Some(input.text.as_str()), commands)
                .map_err(super::session_err)?;
        }
        let harness = self.harness_note(self.work_progress()?.kind);
        input.text = crate::session::decorate(&harness, &input.text);
        let queued = self
            .session
            .start_input(&input.text, &input.images)
            .map_err(super::session_err)?;
        self.history.extend(queued.into_iter().map(Message::user));
        self.history.push(Message::user(input.clone()));
        let mut messages = Vec::with_capacity(self.history.len() + 2);
        messages.push(Message::system(self.system_prompt.clone()));
        messages.extend(self.history.iter().cloned());
        Ok(messages)
    }

    fn round_abort(&self) -> Option<String> {
        self.budget
            .breach(self.spent_tokens, self.started.elapsed().as_secs())
    }

    fn harness_note(&self, kind: crate::WorkKind) -> String {
        let mut note =
            crate::prompt::harness_section(self.plan, self.verify.as_deref().unwrap_or_default());
        if self.plan {
            note.push_str("\n\nHarness: Work remains in Plan until you select and accept it.");
        } else if kind != crate::WorkKind::Single {
            note.push_str("\n\nHarness: the Steps or Goal runner owns this Work.");
        }
        note
    }

    async fn prepare_round(
        &mut self,
        messages: &mut Vec<Message>,
        tools: &[crate::provider::ToolDefinition],
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        self.bus_heartbeat("working");
        self.take_mail(messages, events, emit)?;
        self.auto_compact(messages, tools, usage, events, emit)
            .await
    }

    pub(super) fn record_usage(
        &mut self,
        assistant: &AssistantMessage,
        prefix_hash: String,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        usage.add(&assistant.usage);
        self.spent_tokens +=
            u64::from(assistant.usage.prompt_tokens + assistant.usage.completion_tokens);
        if assistant.usage.prompt_tokens > 0 || assistant.usage.completion_tokens > 0 {
            self.push_event(
                events,
                emit,
                Event::Usage {
                    prompt_tokens: assistant.usage.prompt_tokens,
                    completion_tokens: assistant.usage.completion_tokens,
                    reasoning_tokens: assistant.usage.reasoning_tokens,
                    cached_tokens: assistant.usage.cached_tokens,
                    prefix_hash,
                },
            )?;
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one close path for a finished provider round"
    )]
    async fn finish_turn(
        &mut self,
        message: Message,
        messages: Vec<Message>,
        mut events: Vec<Event>,
        mut emit: impl FnMut(&Event),
        round: usize,
        mut usage: Usage,
        attempt: crate::durable::id::ProviderAttemptId,
        reasoning: String,
        meter: crate::durable::effect::Usage,
    ) -> Result<Turn> {
        if message.content.trim().is_empty() {
            return Err(Error::provider(
                ProviderFailureKind::Malformed,
                "assistant returned neither text nor a tool call",
            ));
        }
        if self.plan {
            let root = self.workspace.join(".mage");
            if root.is_file() {
                return Err(Error::Config("plan directory is not a folder".to_string()));
            }
            self.session
                .finish_plan(
                    attempt,
                    &message.content,
                    &reasoning,
                    meter,
                    self.verify.as_deref().unwrap_or_default(),
                )
                .map_err(super::session_err)?;
            self.commit(&messages);
            let progress = self.work_progress()?;
            self.push_event(&mut events, &mut emit, Event::Work { progress })?;
            let text = message.content;
            self.push_event(
                &mut events,
                &mut emit,
                Event::Assistant { text: text.clone() },
            )?;
            return Ok(Turn {
                text,
                events,
                rounds: round,
                usage,
                verify: None,
                aborted: false,
            });
        }
        self.session
            .finish_provider(attempt, &message.content, &reasoning, meter)
            .map_err(super::session_err)?;
        let (text, verify, receipt) = if self.session.operation_stage()
            == Some(crate::durable::work::WorkStage::Execute)
        {
            let capture = self.session.enter_verify().map_err(super::session_err)?;
            self.session
                .start_verification()
                .map_err(super::session_err)?;
            let (mut checks, evidence) = self.run_verify(&mut events, &mut emit).await?;
            let mut verdict = self
                .session
                .prepare_verdict(evidence, checks.iter().all(|check| check.passed))
                .map_err(super::session_err)?;
            if let Some(output) = verdict.diagnostic.clone() {
                let outcome = Outcome {
                    command: "workspace candidate unchanged".to_string(),
                    passed: false,
                    output,
                };
                self.push_event(
                    &mut events,
                    &mut emit,
                    Event::Verify {
                        command: outcome.command.clone(),
                        passed: false,
                    },
                )?;
                checks.push(outcome);
                verdict.passed = false;
            }
            let receipt = crate::verify::summary(&capture.paths, &checks);
            let decision = if verdict.passed {
                if let Some(objective) =
                    self.session.goal_objective().map_err(super::session_err)?
                {
                    crate::goal::decision(self, &objective, &message.content, &receipt, &mut usage)
                        .await
                } else {
                    crate::durable::driver::VerdictDecision::Automatic
                }
            } else {
                crate::durable::driver::VerdictDecision::Automatic
            };
            let text = crate::verify::receipt(&message.content, &capture.paths, &checks);
            self.session
                .commit_verdict(verdict, decision, Some((&text, capture.provider)))
                .map_err(super::session_err)?;
            (text, checks.last().cloned(), Some(receipt))
        } else {
            self.session.close_operation().map_err(super::session_err)?;
            (message.content, None, None)
        };
        self.commit(&messages);
        let progress = self.work_progress()?;
        self.push_event(&mut events, &mut emit, Event::Work { progress })?;
        if let Some(text) = receipt {
            self.push_event(&mut events, &mut emit, Event::Receipt { text })?;
        }
        let event = Event::Assistant { text: text.clone() };
        self.push_event(&mut events, &mut emit, event)?;
        Ok(Turn {
            text,
            events,
            rounds: round,
            usage,
            verify,
            aborted: false,
        })
    }

    async fn execute_calls(
        &mut self,
        planned: Vec<crate::durable::id::ToolCallId>,
        calls: Vec<ToolCall>,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        for call in &calls {
            let event = Event::ToolCall {
                id: call.id.clone(),
                name: call.function.name.clone(),
                arguments: call.function.arguments.clone(),
            };
            self.push_event(events, emit, event)?;
        }
        let mut finished = Vec::new();
        let mut results = Vec::new();
        for (planned, call) in planned.into_iter().zip(calls) {
            self.session
                .start_tool(planned, &call.function.arguments)
                .map_err(super::session_err)?;
            let started = Instant::now();
            let output = if crate::bus::is_bus_tool(&call.function.name) {
                self.bus_execute(&call)
                    .map(|text| crate::tools::ToolOutput {
                        text,
                        success: true,
                        mutated: false,
                    })
            } else {
                self.tools.execute_outcome(&call).await
            };
            let (success, mutated, text) = match output {
                Ok(output) => (output.success, output.mutated, output.text),
                Err(error) => (false, false, error.to_string()),
            };
            finished.push((
                planned,
                text.clone(),
                success,
                crate::durable::effect::Usage {
                    input_tokens: None,
                    output_tokens: None,
                    cached_input_tokens: None,
                    live_millis: Some(super::elapsed_millis(started)),
                },
            ));
            results.push((call.id, success, mutated, text));
        }
        self.session
            .finish_tools(
                &finished
                    .iter()
                    .map(|(id, text, success, usage)| (*id, text.as_str(), *success, usage.clone()))
                    .collect::<Vec<_>>(),
            )
            .map_err(super::session_err)?;
        for (id, success, mutated, text) in results {
            let event = Event::ToolResult {
                id: id.clone(),
                success,
                mutated,
                output: text.clone(),
            };
            self.push_event(events, emit, event)?;
            messages.push(Message::tool(id, text));
        }
        self.commit(messages);
        Ok(())
    }

    async fn run_verify(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<(Vec<Outcome>, Vec<crate::durable::work::EvidenceRef>)> {
        let declared = self
            .session
            .completion_checks()
            .map_err(super::session_err)?;
        let mut checks = Vec::new();
        let mut evidence = Vec::new();
        for check in declared {
            let command = check.command.as_str().to_string();
            let outcome = match &check.required_observation {
                Some(crate::durable::work::RequiredObservation::File(path)) => {
                    match self.tools.exists(path.as_str()) {
                        Ok(true) => Outcome {
                            command,
                            passed: true,
                            output: format!("file `{}` exists", path.as_str()),
                        },
                        Ok(false) => Outcome {
                            command,
                            passed: false,
                            output: format!("file `{}` does not exist", path.as_str()),
                        },
                        Err(error) => Outcome {
                            command,
                            passed: false,
                            output: error.to_string(),
                        },
                    }
                }
                Some(crate::durable::work::RequiredObservation::StdoutContains(expected)) => {
                    let result = self.tools.execute_outcome(&verify_call(&command)).await;
                    let mut outcome = crate::verify::outcome(&command, &result);
                    if outcome.passed && !outcome.output.contains(expected.as_str()) {
                        outcome.passed = false;
                        outcome.output.push_str(&format!(
                            "\nexpected stdout to contain `{}`",
                            expected.as_str()
                        ));
                    }
                    outcome
                }
                None => {
                    let result = self.tools.execute_outcome(&verify_call(&command)).await;
                    crate::verify::outcome(&command, &result)
                }
            };
            let finding = self
                .session
                .record_completion_check(
                    crate::durable::fact::CompletionCheck::Declared(check.id),
                    &outcome.command,
                    outcome.passed,
                    &outcome.output,
                )
                .map_err(super::session_err)?;
            evidence.push(finding);
            self.push_event(
                events,
                emit,
                Event::Verify {
                    command: outcome.command.clone(),
                    passed: outcome.passed,
                },
            )?;
            let passed = outcome.passed;
            checks.push(outcome);
            if !passed {
                break;
            }
        }
        Ok((checks, evidence))
    }

    fn abort_turn(
        &mut self,
        abort_reason: Option<String>,
        mut events: Vec<Event>,
        mut emit: impl FnMut(&Event),
        usage: Usage,
    ) -> Result<Turn> {
        let reason =
            abort_reason.unwrap_or_else(|| format!("stopped after {} rounds", self.max_rounds));
        self.push_event(
            &mut events,
            &mut emit,
            Event::Aborted {
                reason: reason.clone(),
            },
        )?;
        if self.session.state().operation.is_some() {
            let abort = if reason.contains("budget") {
                AbortReason::Budget
            } else {
                AbortReason::User
            };
            self.session.abort(abort).map_err(super::session_err)?;
        }
        Ok(Turn {
            text: reason,
            events,
            rounds: self.max_rounds,
            usage,
            verify: None,
            aborted: true,
        })
    }

    pub(super) fn commit(&mut self, _messages: &[Message]) {
        self.hydrate();
    }

    fn hydrate(&mut self) {
        let replay = self.session.replay();
        self.history = replay
            .history
            .into_iter()
            .filter(|message| message.role != Role::System)
            .collect();
    }

    pub(super) fn push_event(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        event: Event,
    ) -> Result<()> {
        emit(&event);
        events.push(event);
        Ok(())
    }
}

fn request_chars(messages: &[Message], tools: &[crate::provider::ToolDefinition]) -> usize {
    messages
        .iter()
        .map(super::compaction::message_chars)
        .sum::<usize>()
        + serde_json::to_string(tools).map_or(0, |schema| schema.len())
}

fn verify_call(command: &str) -> ToolCall {
    ToolCall {
        id: "completion-check".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": command,
                "timeout_secs": 600,
                "fresh": true
            })
            .to_string(),
        },
    }
}
