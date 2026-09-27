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
            self.tasks_execute(&calls, events, emit)
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
        } else if calls.iter().all(|call| readonly(&call.function.name)) {
            let mut ran = Vec::with_capacity(calls.len());
            for call in &calls {
                let started = Instant::now();
                ran.push((
                    self.tools.execute_outcome(call).await,
                    super::elapsed_millis(started),
                    None,
                ));
            }
            let mut outputs = Vec::with_capacity(ran.len());
            for ((output, millis, child_usage), call) in ran.into_iter().zip(&calls) {
                outputs.push((
                    self.with_grant(output, call, events, emit).await?,
                    millis,
                    child_usage,
                ));
            }
            outputs
        } else {
            for (planned, call) in planned.into_iter().zip(calls) {
                let started = Instant::now();
                let (output, child_usage) = if Builtin::is(&call.function.name, Builtin::Task) {
                    match self.task_execute(&call, events, emit).await {
                        Ok(execution) => (Ok(execution.output), Some(execution.usage)),
                        Err(error) => (Err(error), None),
                    }
                } else if let Some(hub) =
                    self.mcp.clone().filter(|hub| hub.owns(&call.function.name))
                {
                    (
                        hub.call(
                            &call.function.name,
                            &call.function.arguments,
                            self.tools.is_read_only(),
                        )
                        .await,
                        None,
                    )
                } else if is_mail_tool(&call.function.name) {
                    (self.mail_execute(&call).map(ToolOutput::ok), None)
                } else {
                    let approval =
                        self.manual && Builtin::is(&call.function.name, Builtin::RunCommand);
                    let denied = if approval {
                        let detail =
                            serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                                .ok()
                                .and_then(|value| {
                                    value
                                        .get("command")
                                        .and_then(serde_json::Value::as_str)
                                        .map(str::to_string)
                                })
                                .unwrap_or_else(|| call.function.name.clone());
                        if self.approve(detail, false, events, emit).await? != GrantAnswer::Deny {
                            self.tools.approve_once();
                            None
                        } else {
                            Some(ToolOutput {
                                text: "summonr: the user denied this command".to_string(),
                                success: false,
                                mutated: false,
                                grant: None,
                            })
                        }
                    } else {
                        None
                    };
                    (
                        if let Some(denied) = denied {
                            Ok(denied)
                        } else {
                            self.with_grant(
                                self.tools.execute_outcome(&call).await,
                                &call,
                                events,
                                emit,
                            )
                            .await?
                        },
                        None,
                    )
                };
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
            return Ok(());
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
        let (success, mutated, text) = match output {
            Ok(output) => (output.success, output.mutated, output.text),
            Err(error) => (false, false, error.to_string()),
        };
        let text = self.tool_text(&call, success, text).await;
        self.session
            .finish_tool(planned, text.as_str(), success, mutated, millis)
            .map_err(super::session_err)?;
        self.emit_records(events, emit)?;
        messages.push(Message {
            failed: !success,
            ..Message::tool(call.id, text)
        });
        if success
            && Builtin::is(&call.function.name, Builtin::Todo)
            && let Ok(todos) = crate::tools::todos_from_args(&call.function.arguments)
        {
            let items: Vec<(String, crate::durable::work::StepState)> = todos
                .iter()
                .map(|todo| {
                    (
                        todo.text.clone(),
                        match todo.status {
                            crate::tools::TodoStatus::Done => {
                                crate::durable::work::StepState::Completed
                            }
                            crate::tools::TodoStatus::Active => {
                                crate::durable::work::StepState::Active
                            }
                            crate::tools::TodoStatus::Pending => {
                                crate::durable::work::StepState::Pending
                            }
                        },
                    )
                })
                .collect();
            match self.session.apply_todos(&items) {
                Ok(()) => {
                    let progress = self.work_progress()?;
                    self.push_event(events, emit, Event::Work { progress })?;
                }
                Err(crate::durable::driver::DriverError::NoWork) => {}
                Err(error) => return Err(super::session_err(error)),
            }
        }
        Ok(())
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
            let started = Instant::now();
            let mut wait = 1_u64;
            let mut announced = false;
            let status = loop {
                let status = jobs::status(&self.workspace, &record.id)?;
                if !status.running || started.elapsed().as_secs() >= JOB_WAIT_SECS {
                    break status;
                }
                if !announced {
                    announced = true;
                    self.push_event(
                        events,
                        emit,
                        Event::Note {
                            text: format!("waiting for background {label}"),
                        },
                    )?;
                }
                tokio::time::sleep(crate::limits::seconds(wait)).await;
                wait = (wait * 2).min(10);
            };
            if status.killed {
                continue;
            }
            if status.running {
                running.push(label.clone());
                unsettled.push(record.id.clone());
            }
            let outcome = Outcome {
                command: label,
                passed: status.exit_code == Some(0),
                output: if status.running {
                    format!(
                        "still running after {JOB_WAIT_SECS}s — wait for it or kill it\n{}",
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
