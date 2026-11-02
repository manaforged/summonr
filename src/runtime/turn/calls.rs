use super::*;
use crate::tools::Builtin;

type Ran = (Result<ToolOutput>, u64, Option<Usage>);

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
        if self.interrupted() {
            return self
                .run_serial(planned, calls, messages, events, emit, usage)
                .await;
        }
        if calls.is_empty() {
            return Ok(());
        }
        if calls.iter().all(|call| readonly(&call.function.name)) {
            let outputs = self.run_readonly(&calls, events, emit).await?;
            let batch = planned.into_iter().zip(calls).zip(outputs).collect();
            return self
                .finish_batch(batch, messages, events, emit, usage)
                .await;
        }
        let is_task = |call: &ToolCall| Builtin::is(&call.function.name, Builtin::Task);
        let mut pending = planned.into_iter().zip(calls).peekable();
        while let Some((planned, call)) = pending.next() {
            if !is_task(&call) || self.interrupted() {
                self.run_serial(vec![planned], vec![call], messages, events, emit, usage)
                    .await?;
                continue;
            }
            let mut tasks = vec![(planned, call)];
            while let Some(next) = pending.next_if(|(_, call)| is_task(call)) {
                tasks.push(next);
            }
            let calls: Vec<ToolCall> = tasks.iter().map(|(_, call)| call.clone()).collect();
            let outputs = self.run_tasks(&calls, events, emit).await;
            let batch = tasks.into_iter().zip(outputs).collect();
            self.finish_batch(batch, messages, events, emit, usage)
                .await?;
        }
        Ok(())
    }

    async fn finish_batch(
        &mut self,
        batch: Vec<((Call, ToolCall), Ran)>,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<()> {
        for ((planned, call), (output, millis, child_usage)) in batch {
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
    ) -> Vec<Ran> {
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
    ) -> Result<Vec<Ran>> {
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
            let mut usage = Usage::default();
            outputs.push((
                self.with_grant(output, call, events, emit, &mut usage)
                    .await?,
                millis,
                Some(usage),
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

    async fn mcp_dispatch(
        &mut self,
        hub: &crate::mcp::Hub,
        call: &ToolCall,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Result<ToolOutput>> {
        let resolved = hub.resolve(
            &call.function.name,
            &call.function.arguments,
            self.tools.policy(),
        );
        let (alias, arguments) = match resolved {
            Ok(crate::mcp::Resolved::Call { alias, arguments }) => (alias, arguments),
            Ok(crate::mcp::Resolved::Found(text)) => {
                return Ok(self.tools.fit_output(Ok(ToolOutput::ok(text))));
            }
            Err(error) => return Ok(Err(error)),
        };
        let policy = self.tools.policy().clone();
        let (sender, mut updates) = tokio::sync::mpsc::unbounded_channel();
        let mut report = move |update| drop(sender.send(update));
        let running = hub.call(&alias, arguments, &policy, &mut report);
        let input = self.input_wait();
        let interrupts = self.input_interrupts();
        tokio::pin!(running, input);
        let output = loop {
            tokio::select! {
                biased;
                result = &mut input, if interrupts => {
                    result?;
                    break Err(Error::Tool("MCP call interrupted by an incoming message; the server may have completed changes".to_string()));
                }
                Some(update) = updates.recv() => self.heard(&call.id, update, events, emit).await?,
                output = &mut running => break output,
            }
        };
        while let Ok(update) = updates.try_recv() {
            self.heard(&call.id, update, events, emit).await?;
        }
        Ok(self.tools.fit_output(output))
    }

    fn child_refusal(&self, name: &str) -> Option<String> {
        if self.session.depth() == 0 {
            return None;
        }
        match Builtin::of(name) {
            None => Some(format!(
                "child sessions cannot call `{name}`; use the built-in tools"
            )),
            Some(Builtin::Edit | Builtin::Write) if self.tools.policy().is_read_only() => {
                Some("this child is read-only; report what to change instead".to_string())
            }
            Some(_) => None,
        }
    }

    async fn dispatch(
        &mut self,
        call: &ToolCall,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<(Result<ToolOutput>, Option<Usage>)> {
        if self.interrupted() {
            return Ok((
                Err(Error::Tool(
                    self.steered("not run: interrupted by an incoming message"),
                )),
                None,
            ));
        }
        let name = &call.function.name;
        self.mail_note_tool(name);
        if Builtin::is(name, Builtin::Task) {
            return Ok(match self.task_execute(call, events, emit).await {
                Ok(execution) => (Ok(execution.output), Some(execution.usage)),
                Err(error) => (Err(error), None),
            });
        }
        if name == super::ask::ASK && self.ask_available() {
            return Ok((self.ask(call, events, emit).await?, None));
        }
        if let Some(refusal) = self.child_refusal(name) {
            return Ok((Err(crate::Error::Denied(refusal)), None));
        }
        if let Some(hub) = self.mcp.clone().filter(|hub| hub.owns(name)) {
            return Ok((self.mcp_dispatch(&hub, call, events, emit).await?, None));
        }
        if is_mail_tool(name) {
            return Ok((self.mail_execute(call).map(ToolOutput::ok), None));
        }
        let (output, mut usage) = self.run_or_wake(call, events, emit).await?;
        Ok((
            self.with_grant(output, call, events, emit, &mut usage)
                .await?,
            Some(usage),
        ))
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
        let mut text = self.tool_text(&call, success, text).await;
        if let Some(nudge) = super::repeat::nudge(messages, &call.function, &text) {
            text.push_str(&nudge);
            self.push_event(
                events,
                emit,
                Event::Note {
                    text: format!("{} keeps returning the same result", call.function.name),
                },
            )?;
        }
        self.session
            .finish_tool(planned, text.as_str(), success, mutated, millis, patch)?;
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
        match self.session.apply_todos(todos) {
            Ok(()) => {
                let progress = self.work_progress()?;
                self.push_event(events, emit, Event::Work { progress })
            }
            Err(crate::durable::driver::DriverError::NoWork) => Ok(()),
            Err(error) => Err(Error::from(error)),
        }
    }
}
