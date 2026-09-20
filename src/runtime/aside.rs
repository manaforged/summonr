use super::*;
use crate::workflow::Role;

impl<P: Provider> Loop<P> {
    pub(crate) async fn aside(
        &mut self,
        role: Role,
        system: &str,
        user: &str,
    ) -> Result<(String, Usage)> {
        let tools = self.advertised_tools().to_vec();
        let plan = self.is_plan();
        self.tools.set_read_only(true);
        let result = self.aside_rounds(role, system, user, &tools).await;
        self.tools.set_read_only(plan);
        result
    }

    async fn aside_rounds(
        &mut self,
        role: Role,
        system: &str,
        user: &str,
        tools: &[ToolDefinition],
    ) -> Result<(String, Usage)> {
        let mut messages = vec![
            Message::system(system.to_string()),
            Message::user(user.to_string()),
        ];
        let mut usage = Usage::default();
        loop {
            if let Some(reason) = self.round_abort() {
                return Err(Error::Config(reason));
            }
            let request = CompletionRequest {
                session: self.session_id(),
                model: self.seat(role).1.to_string(),
                messages: messages.clone(),
                tools: tools.to_vec(),
                max_output_tokens: None,
                emit_deltas: false,
                effort: self.effort_for(role),
            };
            drop(request.prefix_hash());
            self.freeze_catalog();
            let durable = request.durable_bytes().map_err(|error| {
                Error::Config(format!("cannot encode provider request: {error}"))
            })?;
            let attempt = self
                .session
                .start_aside(role, &request.model, super::exposure(&request, &durable))
                .map_err(super::session_err)?;
            let started = Instant::now();
            let completion = self.seat(role).0.complete(&request);
            let assistant = match Self::wait(completion, self.remaining_time()).await {
                Ok(assistant) => assistant,
                Err(wait_error) => {
                    let millis = wait_error.millis(started);
                    let error = wait_error.error();
                    self.session
                        .fail_provider(attempt, &error.to_string(), millis, super::billable(&error))
                        .map_err(super::session_err)?;
                    self.spent_tokens = self.session.replay().spent_tokens;
                    return Err(error);
                }
            };
            let text = assistant.content.clone().unwrap_or_default();
            let meter = super::meter(&assistant.usage, super::elapsed_millis(started));
            self.session
                .finish_provider(attempt, &text, "", meter)
                .map_err(super::session_err)?;
            usage.add(&assistant.usage);
            self.spent_tokens = self.spent_tokens.saturating_add(u64::from(
                assistant
                    .usage
                    .prompt_tokens
                    .saturating_add(assistant.usage.completion_tokens),
            ));
            if let Some(reason) = self.round_abort() {
                return Err(Error::Config(reason));
            }
            let message = assistant.into_message();
            let calls = message.tool_calls.clone();
            messages.push(message);
            if calls.is_empty() {
                return Ok((text, usage));
            }
            for call in calls {
                let (success, output) = match self.tools.execute_outcome(&call).await {
                    Ok(output) => (output.success, output.text),
                    Err(error) => (false, error.to_string()),
                };
                let text = self.tool_text(&call, success, output).await;
                messages.push(Message::tool(call.id, text));
            }
        }
    }
}
