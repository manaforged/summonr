use super::*;
use crate::tools::Builtin;

impl<P: Provider> Loop<P> {
    pub(super) async fn execute_calls(
        &mut self,
        planned: Vec<Call>,
        calls: Vec<ToolCall>,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<()> {
        let (planned, calls) = self.screen(planned, calls, messages, events, emit).await?;
        if calls.is_empty() {
            return Ok(());
        }
        let outputs = if calls
            .iter()
            .all(|call| Builtin::is(&call.function.name, Builtin::Task))
        {
            self.run_tasks(&calls, events, emit).await
        } else if calls.iter().all(|call| readonly(&call.function.name)) {
            self.run_readonly(&calls, events, emit).await?
        } else {
            return self
                .run_serial(planned, calls, messages, events, emit, usage)
                .await;
        };
        for ((planned, call), (output, millis, child_usage)) in
            planned.into_iter().zip(calls).zip(outputs)
        {
            if let Some(child_usage) = child_usage {
                usage.add(&child_usage);
            }
            self.finish_call(planned, call, (output, millis), messages, events, emit)
                .await?;
        }
        Ok(())
    }

    async fn run_tasks(
        &mut self,
        calls: &[ToolCall],
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Vec<(Result<ToolOutput>, u64, Option<Usage>)> {
        self.tasks_execute(calls, events, emit)
            .await
            .into_iter()
            .map(|result| match result {
                Ok(execution) => (
                    Ok(execution.output),
                    execution.millis,
                    Some(execution.usage),
                ),
                Err(error) => (Err(error), 0, None),
            })
            .collect()
    }

    async fn run_readonly(
        &mut self,
        calls: &[ToolCall],
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Vec<(Result<ToolOutput>, u64, Option<Usage>)>> {
        let tools = &self.tools;
        let ran = calls
            .iter()
            .map(|call| async move {
                let started = Instant::now();
                (
                    tools.execute_outcome(call).await,
                    super::elapsed_millis(started),
                )
            })
            .collect::<Vec<_>>();
        let ran = futures::future::join_all(ran).await;
        let mut outputs = Vec::with_capacity(ran.len());
        for ((output, millis), call) in ran.into_iter().zip(calls) {
            outputs.push((
                self.with_grant(output, call, events, emit).await?,
                millis,
                None,
            ));
        }
        Ok(outputs)
    }

    async fn run_serial(
        &mut self,
        planned: Vec<Call>,
        calls: Vec<ToolCall>,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<()> {
        for (planned, call) in planned.into_iter().zip(calls) {
            let started = Instant::now();
            let (output, child_usage) = self.dispatch(&call, events, emit).await?;
            if let Some(child_usage) = child_usage {
                usage.add(&child_usage);
            }
            self.finish_call(
                planned,
                call,
                (output, super::elapsed_millis(started)),
                messages,
                events,
                emit,
            )
            .await?;
        }
        Ok(())
    }

    async fn dispatch(
        &mut self,
        call: &ToolCall,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<(Result<ToolOutput>, Option<Usage>)> {
        let name = &call.function.name;
        if Builtin::is(name, Builtin::Task) {
            return Ok(match self.task_execute(call, events, emit).await {
                Ok(execution) => (Ok(execution.output), Some(execution.usage)),
                Err(error) => (Err(error), None),
            });
        }
        if let Some(hub) = self.mcp.clone().filter(|hub| hub.owns(name)) {
            let resolved = hub.resolve(name, &call.function.arguments, self.tools.policy());
            let (alias, arguments) = match resolved {
                Ok(crate::mcp::Resolved::Call { alias, arguments }) => (alias, arguments),
                Ok(crate::mcp::Resolved::Found(text)) => {
                    return Ok((self.tools.fit_output(Ok(ToolOutput::ok(text))), None));
                }
                Err(error) => return Ok((Err(error), None)),
            };
            let sandbox = self.tools.sandbox();
            let policy = self.tools.policy().clone();
            let (sender, mut updates) = tokio::sync::mpsc::unbounded_channel();
            let mut report = move |update| drop(sender.send(update));
            let running = hub.call(&alias, arguments, &policy, sandbox.as_ref(), &mut report);
            tokio::pin!(running);
            let output = loop {
                tokio::select! {
                    biased;
                    Some(update) = updates.recv() => self.heard(&call.id, update, events, emit).await?,
                    output = &mut running => break output,
                }
            };
            while let Ok(update) = updates.try_recv() {
                self.heard(&call.id, update, events, emit).await?;
            }
            return Ok((self.tools.fit_output(output), None));
        }
        if is_mail_tool(name) {
            return Ok((self.mail_execute(call).map(ToolOutput::ok), None));
        }
        let output = self.tools.execute_outcome(call).await;
        Ok((self.with_grant(output, call, events, emit).await?, None))
    }

    async fn heard(
        &mut self,
        id: &str,
        update: crate::mcp::McpUpdate,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        match update {
            crate::mcp::McpUpdate::Progress(progress) => emit(&Event::Progress {
                id: id.to_string(),
                progress: progress.progress,
                total: progress.total,
                message: progress.message,
            }),
            crate::mcp::McpUpdate::Elicit { form, answer } => {
                answer.give(&self.elicit(form, events, emit).await?);
            }
        }
        Ok(())
    }

    pub(in crate::runtime) async fn finish_call(
        &mut self,
        planned: Call,
        call: ToolCall,
        result: (Result<ToolOutput>, u64),
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let (output, millis) = result;
        let (success, mutated, text, images, patch) = match output {
            Ok(output) => (
                output.success,
                output.mutated,
                output.text,
                output.images,
                output.patch,
            ),
            Err(error) => (false, false, error.to_string(), Vec::new(), None),
        };
        let text = self.tool_text(&call, success, text).await;
        self.session
            .finish_tool(planned, text.as_str(), success, mutated, millis, patch)
            .map_err(super::session_err)?;
        self.emit_records(events, emit)?;
        let todo = success && Builtin::is(&call.function.name, Builtin::Todo);
        let arguments = call.function.arguments.clone();
        messages.push(Message {
            failed: !success,
            images,
            ..Message::tool(call.id, text)
        });
        if todo {
            self.record_todos(&arguments, events, emit)?;
        }
        Ok(())
    }

    fn record_todos(
        &mut self,
        arguments: &str,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let Ok(todos) = crate::tools::todos_from_args(arguments) else {
            return Ok(());
        };
        let items: Vec<(String, crate::durable::work::StepState)> = todos
            .iter()
            .map(|todo| (todo.text.clone(), step_state(todo.status)))
            .collect();
        match self.session.apply_todos(&items) {
            Ok(()) => {
                let progress = self.work_progress()?;
                self.push_event(events, emit, Event::Work { progress })
            }
            Err(crate::durable::driver::DriverError::NoWork) => Ok(()),
            Err(error) => Err(super::session_err(error)),
        }
    }

    pub(super) async fn await_jobs(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<(Vec<Outcome>, Vec<String>, Vec<String>)> {
        let mut outcomes: Vec<(String, Outcome)> = Vec::new();
        let mut running = Vec::new();
        let mut unsettled = Vec::new();
        let session = self.session.id().to_string();
        let workspace = self.workspace.clone();
        let mut owned: BTreeMap<String, jobs::Record> = BTreeMap::new();
        for id in self.session.jobs() {
            if let Ok(record) = jobs::load(&self.workspace, &id) {
                owned.insert(record.id.clone(), record);
            }
        }
        for record in jobs::since(&self.workspace, self.turn_started, Some(&session)) {
            owned.insert(record.id.clone(), record);
        }
        for record in owned.into_values() {
            let label = format!("job {} `{}`", record.id, record.command);
            let status = jobs::wait(&workspace, &record.id, JOB_WAIT_SECS, || {
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("waiting for background {label}"),
                    },
                )
            })
            .await?;
            if status.state == jobs::JobState::Killed {
                continue;
            }
            let running_now = status.state == jobs::JobState::Running;
            if running_now {
                running.push(label.clone());
                unsettled.push(record.id.clone());
            }
            let outcome = Outcome {
                command: label,
                passed: status.state == jobs::JobState::Exited(0),
                output: if running_now {
                    format!(
                        "still running after {JOB_WAIT_SECS}s; wait for it or kill it\n{}",
                        status.text
                    )
                } else {
                    status.text
                },
            };
            match outcomes
                .iter_mut()
                .find(|(command, _)| *command == record.command)
            {
                Some((_, latest)) => *latest = outcome,
                None => outcomes.push((record.command, outcome)),
            }
        }
        for (_, outcome) in &outcomes {
            self.push_event(
                events,
                emit,
                Event::Verify {
                    command: outcome.command.clone(),
                    passed: outcome.passed,
                },
            )?;
        }
        Ok((
            outcomes.into_iter().map(|(_, outcome)| outcome).collect(),
            running,
            unsettled,
        ))
    }
}

fn step_state(status: crate::tools::TodoStatus) -> crate::durable::work::StepState {
    match status {
        crate::tools::TodoStatus::Done => crate::durable::work::StepState::Completed,
        crate::tools::TodoStatus::Active => crate::durable::work::StepState::Active,
        crate::tools::TodoStatus::Pending => crate::durable::work::StepState::Pending,
    }
}
