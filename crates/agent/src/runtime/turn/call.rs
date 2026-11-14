use super::*;

#[derive(Clone, Copy)]
pub(in crate::runtime) enum CallSource<'a> {
    History(&'a mail::Advertised),
    Slice {
        messages: &'a [Message],
        advertised: Option<&'a mail::Advertised>,
    },
}

struct CallPlan {
    adjusted_system: Option<String>,
    strip_leading: bool,
    prefix_hash: String,
    chars: usize,
    exposure: Exposure,
    model: String,
}

impl<P: Provider> Loop<P> {
    fn plan_call(
        &self,
        role: workflow::Role,
        source: CallSource<'_>,
        stream: bool,
        images: bool,
    ) -> CallPlan {
        let (_, model) = self.selection(role);
        let mut request = assemble_request(
            self.session_id(),
            &self.system_prompt,
            model,
            self.effort_for(role),
            self.session.history(),
            source,
            stream,
            None,
            false,
        );
        let (adjusted_system, strip_leading) = adjust_request_system(&mut request, images);
        let prefix_hash = request.prefix_hash();
        let request = request.with_prefix_hash(prefix_hash.clone());
        let tool_bytes = request.tools_json.map_or_else(
            || crate::provider::ToolsJson::encode(request.tools).len(),
            crate::provider::ToolsJson::len,
        );
        let chars = request_chars(
            &request,
            tool_bytes,
            self.selection(role).0.keeps_reasoning(),
        );
        let exposure = super::super::exposure(&request, chars);
        CallPlan {
            adjusted_system,
            strip_leading,
            prefix_hash,
            chars,
            exposure,
            model: request.model,
        }
    }

    pub(in crate::runtime) async fn call(
        &mut self,
        role: workflow::Role,
        source: CallSource<'_>,
        options: CallOptions,
    ) -> Result<std::result::Result<Reply, Missed>> {
        let images = self.selection(role).0.takes_images();
        self.tools.set_model_images(images);
        if options.retry == RetryPolicy::None
            && let Some(reason) = self.round_abort()
        {
            return Err(Error::Config(reason));
        }
        let plan = self.plan_call(role, source, options.stream, images);
        let attempt = self.open_call_round(role, &plan)?;
        let started = Instant::now();
        let remaining = self.remaining_time();
        let input = self.input_wait();
        let session_id = self.session_id();
        let effort = self.effort_for(role);
        let (history, partial) = self.session.history_and_partial();
        let (provider, model_name) =
            super::super::selected(&self.selections, &self.provider, &self.model, role);
        let request = assemble_request(
            session_id,
            &self.system_prompt,
            model_name,
            effort,
            history,
            source,
            options.stream,
            plan.adjusted_system.as_deref(),
            plan.strip_leading,
        )
        .with_prefix_hash(plan.prefix_hash.clone());
        let completion = if options.stream {
            provider.stream(&request, partial)
        } else {
            provider.complete(&request)
        };
        let waited = tokio::select! {
            biased;
            result = Self::wait(completion, remaining) => result,
            result = input, if options.retry == RetryPolicy::Round => {
                Err(result.err().map_or(WaitError::Steered, WaitError::Provider))
            }
        };
        let message = match waited {
            Ok(message) => message,
            Err(error) => {
                self.record_call_failure(attempt, &error, started)?;
                return Ok(Err(Missed { error }));
            }
        };
        self.record_call_reply(role, attempt, options.retry, plan, started, message)
    }

    fn open_call_round(
        &mut self,
        role: workflow::Role,
        plan: &CallPlan,
    ) -> Result<Option<Attempt>> {
        if !self.session.turn_open() {
            return Ok(None);
        }
        Ok(Some(self.session.start_round(
            purpose_of(role),
            &plan.model,
            Exposure {
                max_input_tokens: plan.exposure.max_input_tokens,
                max_output_tokens: plan.exposure.max_output_tokens,
            },
        )?))
    }

    fn record_call_failure(
        &mut self,
        attempt: Option<Attempt>,
        error: &WaitError,
        started: Instant,
    ) -> Result<()> {
        let Some(attempt) = attempt else {
            return Ok(());
        };
        let millis = error.millis(started);
        let (diagnostic, billable) = match error {
            WaitError::Deadline(_) => (Abort::time_limit().reason, true),
            WaitError::Steered => ("interrupted by an incoming message".to_string(), true),
            WaitError::Provider(error) => (error.to_string(), super::super::billable(error)),
        };
        self.session
            .fail_provider(attempt, &diagnostic, millis, billable)?;
        Ok(())
    }

    fn record_call_reply(
        &mut self,
        role: workflow::Role,
        attempt: Option<Attempt>,
        retry: RetryPolicy,
        plan: CallPlan,
        started: Instant,
        message: AssistantMessage,
    ) -> Result<std::result::Result<Reply, Missed>> {
        self.session.partial = Default::default();
        let meter = Meter::measured(&message.usage, super::super::elapsed_millis(started));
        let meta = UsageMeta {
            continuation: message.continuation.clone(),
            prefix_hash: plan.prefix_hash,
            cost: self
                .price_for(role)
                .map(|price| price.cost(meter.input, meter.output, meter.cached, meter.written)),
        };
        let open = match (attempt, retry) {
            (Some(attempt), RetryPolicy::Round) => Some((attempt, meter, meta)),
            (Some(attempt), RetryPolicy::None) => {
                self.session.finish_provider(
                    attempt,
                    message.content.as_deref().unwrap_or(""),
                    message.reasoning_content.as_deref().unwrap_or(""),
                    meter,
                    meta,
                )?;
                None
            }
            (None, _) => {
                self.session
                    .charge(meter, "outside a turn", &plan.model, true, meta)?;
                None
            }
        };
        Ok(Ok(Reply {
            message,
            chars: plan.chars,
            open,
        }))
    }
}

fn purpose_of(role: workflow::Role) -> Purpose {
    match role {
        workflow::Role::Implement => Purpose::Agent,
        workflow::Role::Summary => Purpose::Summary,
        workflow::Role::Judge => Purpose::Judge,
        workflow::Role::Scout => Purpose::Scout,
        workflow::Role::Title => Purpose::Title,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "disjoint Loop fields avoid cloning history"
)]
fn assemble_request<'a>(
    session: String,
    base_system: &'a str,
    model: &str,
    effort: Option<Effort>,
    history: &'a [Message],
    source: CallSource<'a>,
    stream: bool,
    adjusted_system: Option<&'a str>,
    strip_leading: bool,
) -> CompletionRequest<'a> {
    let (messages, tools, tools_json, default_system) = match source {
        CallSource::History(advertised) => (
            super::super::trim_trailing_assistant(history),
            advertised.tools.as_slice(),
            Some(&advertised.json),
            Some(base_system),
        ),
        CallSource::Slice {
            messages,
            advertised,
        } => (
            if strip_leading {
                &messages[1..]
            } else {
                messages
            },
            advertised.map_or(&[][..], |advertised| advertised.tools.as_slice()),
            advertised.map(|advertised| &advertised.json),
            None,
        ),
    };
    let mut request = CompletionRequest::new(
        session,
        model.to_string(),
        messages,
        tools,
        None,
        stream,
        effort,
    );
    if let Some(json) = tools_json {
        request = request.with_tools_json(json);
    }
    if let Some(system) = adjusted_system.or(default_system) {
        request = request.with_system_prompt(system);
    }
    request
}

fn adjust_request_system(
    request: &mut CompletionRequest<'_>,
    images: bool,
) -> (Option<String>, bool) {
    let source = request.system_prompt.as_deref().or_else(|| {
        request
            .messages
            .first()
            .filter(|message| message.role == Role::System)
            .map(|message| message.content.as_str())
    });
    let Some(source) = source else {
        return (None, false);
    };
    if let std::borrow::Cow::Owned(updated) = crate::prompt::model_system(source, images) {
        let strip_leading = request.system_prompt.is_none();
        if strip_leading {
            request.messages = &request.messages[1..];
        }
        request.system_prompt = Some(std::borrow::Cow::Owned(updated.clone()));
        return (Some(updated), strip_leading);
    }
    (None, false)
}

fn request_chars(
    request: &CompletionRequest<'_>,
    tool_bytes: usize,
    reasoning_on_wire: bool,
) -> usize {
    request.system_prompt.as_deref().map_or(0, str::len)
        + request
            .messages
            .iter()
            .map(|message| super::super::compaction::message_chars(message, reasoning_on_wire))
            .sum::<usize>()
        + tool_bytes
}
