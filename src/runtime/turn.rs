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
        let (mail, overflow) = self.take_bus_mail(5);
        for mail in mail {
            let text = crate::bus::envelope(&mail.from, &mail.text);
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
    ) -> Result<(
        AssistantMessage,
        String,
        usize,
        crate::durable::id::ProviderAttemptId,
    )> {
        let mut attempt = 0_u32;
        let mut overflowed = false;
        let mut wait = 2_u64;
        loop {
            let request = self.completion_request(messages, tools);
            let prefix_hash = request.prefix_hash();
            let request_chars = request_chars(messages, tools);
            let durable_attempt = self
                .session
                .start_provider(prefix_hash.as_bytes())
                .map_err(super::session_err)?;
            match self.provider.complete(&request).await {
                Ok(message) => return Ok((message, prefix_hash, request_chars, durable_attempt)),
                Err(error) => {
                    self.session
                        .fail_provider(
                            durable_attempt,
                            durable_failure(error.provider_failure()),
                            &error.to_string(),
                        )
                        .map_err(super::session_err)?;
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
        let harness = self.harness_note();
        self.session
            .start_operation(&harness)
            .map_err(super::session_err)?;
        self.history.push(Message::user(harness));
        let mut messages = Vec::with_capacity(self.history.len() + 2);
        messages.push(Message::system(self.system_prompt.clone()));
        messages.extend(self.history.iter().cloned());
        self.run_turn(messages, emit).await
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
            let (assistant, prefix_hash, request_chars, durable_attempt) = self
                .complete_retry(&mut messages, &tools, &mut events, &mut emit)
                .await?;
            if assistant.usage.prompt_tokens > 0 {
                self.context_sample = Some((request_chars, assistant.usage.prompt_tokens));
            }
            self.record_usage(&assistant, prefix_hash, &mut usage, &mut events, &mut emit)?;
            let stop_reason = assistant.stop_reason;
            let meter = super::meter(&assistant.usage);
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
                if self.task_enabled
                    && !self.plan
                    && self.task.as_ref().is_some_and(|task| !task.is_complete())
                {
                    self.session
                        .finish_provider(durable_attempt, &message.content, &reasoning, meter)
                        .map_err(super::session_err)?;
                    self.continue_incomplete_task(message, &mut messages, &mut events, &mut emit)?;
                    continue;
                }
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
        let harness = self.harness_note();
        input.text = crate::session::decorate(&harness, &input.text);
        self.session
            .start_input(&input.text, &input.images)
            .map_err(super::session_err)?;
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

    fn harness_note(&self) -> String {
        let mut note =
            crate::prompt::harness_section(self.plan, self.verify.as_deref().unwrap_or_default());
        if self.task_enabled && !self.plan {
            note.push_str(
                "\n\nHarness: before edit_file or write_file, start task_state in the same response. Advance each completed step.",
            );
        } else if self.plan {
            note.push_str("\n\nHarness: task_state is suspended in plan mode.");
        } else if !self.task_enabled {
            note.push_str(
                "\n\nHarness: the external step runner owns progress. Do not call task_state.",
            );
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
        usage: Usage,
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
            if self.session.state().works.is_empty() {
                self.session
                    .set_work(&message.content)
                    .map_err(super::session_err)?;
            }
            self.session
                .select_plan(&message.content)
                .map_err(super::session_err)?;
        }
        self.session
            .finish_provider(attempt, &message.content, &reasoning, meter)
            .map_err(super::session_err)?;
        let event = Event::Assistant {
            text: message.content.clone(),
        };
        self.push_event(&mut events, &mut emit, event)?;
        self.commit(&messages);
        self.session.close_operation().map_err(super::session_err)?;
        Ok(Turn {
            text: message.content,
            events,
            rounds: round,
            usage,
            verify: None,
            aborted: false,
        })
    }

    fn continue_incomplete_task(
        &mut self,
        message: Message,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        self.push_event(
            events,
            emit,
            Event::Assistant {
                text: message.content,
            },
        )?;
        let current = self
            .task
            .as_ref()
            .and_then(crate::task::TaskState::current)
            .unwrap_or("the current step")
            .to_string();
        let control = UserInput::from(format!(
            "[summon control: the task checklist is incomplete. Continue with `{current}`, then call task_state advance. Do not give the final answer yet.]"
        ));
        self.session
            .steer(
                &control.text,
                crate::durable::context::InputOrigin::HarnessControl,
            )
            .map_err(super::session_err)?;
        messages.push(Message::user(control));
        self.push_event(
            events,
            emit,
            Event::Note {
                text: format!("task incomplete — continuing with {current}"),
            },
        )?;
        self.commit(messages);
        Ok(())
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
                .start_tool(planned, &call.function.arguments, &call.function.name)
                .map_err(super::session_err)?;
            let (output, next_task) = if call.function.name == "task_state" {
                if !self.task_enabled || self.plan {
                    (
                        Err(Error::Denied(
                            "task_state is unavailable in the current mode".to_string(),
                        )),
                        None,
                    )
                } else {
                    match crate::task::TaskState::transition(
                        self.task.as_ref(),
                        &call.function.arguments,
                    ) {
                        Ok((task, text)) => (
                            Ok(crate::tools::ToolOutput {
                                text,
                                success: true,
                                mutated: false,
                            }),
                            Some(task),
                        ),
                        Err(error) => (Err(error), None),
                    }
                }
            } else if self.task_enabled
                && !self.plan
                && crate::task::guards(&call.function.name)
                && self
                    .task
                    .as_ref()
                    .is_none_or(crate::task::TaskState::is_complete)
            {
                (
                    Err(Error::Denied(format!(
                        "start task_state before using {}",
                        call.function.name
                    ))),
                    None,
                )
            } else if call.function.name == "agent" {
                if self.plan {
                    (
                        Err(Error::Denied(
                            "agent is unavailable in plan mode".to_string(),
                        )),
                        None,
                    )
                } else {
                    (self.run_nested(&call.function.arguments).await, None)
                }
            } else if crate::bus::is_bus_tool(&call.function.name) {
                (
                    self.bus_execute(&call)
                        .map(|text| crate::tools::ToolOutput {
                            text,
                            success: true,
                            mutated: false,
                        }),
                    None,
                )
            } else {
                (self.tools.execute_outcome(&call).await, None)
            };
            let (success, mutated, text) = match output {
                Ok(output) => (output.success, output.mutated, output.text),
                Err(error) => (false, false, error.to_string()),
            };
            finished.push((
                planned,
                text.clone(),
                crate::durable::effect::Usage {
                    input_tokens: None,
                    output_tokens: None,
                    cached_input_tokens: None,
                    live_millis: None,
                },
            ));
            results.push((call.id, success, mutated, next_task, text));
        }
        self.session
            .finish_tools(
                &finished
                    .iter()
                    .map(|(id, text, usage)| (*id, text.as_str(), usage.clone()))
                    .collect::<Vec<_>>(),
            )
            .map_err(super::session_err)?;
        for (id, success, mutated, next_task, text) in results {
            let event = Event::ToolResult {
                id: id.clone(),
                success,
                mutated,
                task: next_task.clone(),
                output: text.clone(),
            };
            self.push_event(events, emit, event)?;
            if success && let Some(task) = next_task {
                self.task = Some(task);
            }
            messages.push(Message::tool(id, text));
        }
        self.commit(messages);
        Ok(())
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
        if let Some(task) = crate::task::from_history(&self.history) {
            self.task = Some(task);
        }
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

fn durable_failure(
    failure: Option<&crate::error::ProviderFailure>,
) -> crate::durable::effect::ProviderFailureKind {
    use crate::durable::effect::ProviderFailureKind as Kind;
    match failure.map(|failure| failure.kind) {
        Some(ProviderFailureKind::Authentication) => Kind::Authentication,
        Some(ProviderFailureKind::RateLimited) => Kind::RateLimit,
        Some(ProviderFailureKind::ContextOverflow) => Kind::ContextOverflow,
        Some(ProviderFailureKind::Transport | ProviderFailureKind::Server) => Kind::Transport,
        _ => Kind::Other,
    }
}

fn request_chars(messages: &[Message], tools: &[crate::provider::ToolDefinition]) -> usize {
    messages
        .iter()
        .map(super::compaction::message_chars)
        .sum::<usize>()
        + serde_json::to_string(tools).map_or(0, |schema| schema.len())
}
