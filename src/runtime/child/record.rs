use super::*;

pub(super) async fn killed(spawned: Spawned, parent: &Path, reason: &str) -> Result<Done> {
    spawned.handle.abort();
    let mut record = spawned.record;
    let mut diagnostic = reason.to_string();
    if let Ok(done) = spawned.handle.await {
        return Ok(done);
    }
    if record.access == Access::Implement && record.workspace != parent && record.workspace.exists()
    {
        diagnostic.push_str(&format!(
            "; the delta at {} holds changes and was kept",
            record.workspace.display()
        ));
    }
    let (billed, usage) = accounting_at(&spawned.path)?;
    let mut result = empty(Some(&diagnostic));
    result.billed_tokens = billed;
    result.usage = usage.clone();
    record.state = ChildState::Aborted(result);
    Ok(Done {
        record,
        tree: None,
        paths: Vec::new(),
        usage,
        billed,
        settled: false,
        millis: super::elapsed_millis(spawned.started),
    })
}

pub(super) fn accounting_at(path: &Path) -> Result<(u64, Usage)> {
    let mut state = State::default();
    let mut usage = Usage::default();
    let lines = log::read(path)
        .map_err(|error| Error::Config(format!("cannot read child session log: {error}")))?;
    for line in lines {
        match &line {
            log::Line::Assistant { usage: meter, .. }
            | log::Line::Charge {
                usage: meter,
                reported: true,
                ..
            } => {
                usage.prompt_tokens = usage
                    .prompt_tokens
                    .saturating_add(u32::try_from(meter.input).unwrap_or(u32::MAX));
                usage.completion_tokens = usage
                    .completion_tokens
                    .saturating_add(u32::try_from(meter.output).unwrap_or(u32::MAX));
                usage.cached_tokens = usage
                    .cached_tokens
                    .saturating_add(u32::try_from(meter.cached).unwrap_or(u32::MAX));
            }
            _ => {}
        }
        state.apply(line);
    }
    Ok((state.spent, usage))
}

pub(super) fn empty(diagnostic: Option<&str>) -> ChildResult {
    ChildResult {
        text: String::new(),
        changed_paths: Vec::new(),
        changed_paths_total: 0,
        rounds: 0,
        usage: Usage::default(),
        billed_tokens: 0,
        elapsed_millis: 0,
        diagnostic: diagnostic.map(str::to_string),
    }
}

impl ChildRecord {
    pub(super) fn note_record(&mut self, diagnostic: &str) {
        if let Some(result) = result_mut(&mut self.state) {
            result.diagnostic = Some(diagnostic.to_string());
        }
    }

    pub(super) fn fail_record(&mut self, diagnostic: &str) {
        let state = std::mem::replace(&mut self.state, ChildState::Running);
        let mut result = match state {
            ChildState::Completed(result)
            | ChildState::Aborted(result)
            | ChildState::Failed(result)
            | ChildState::Interrupted(result) => result,
            ChildState::Running => return,
        };
        result.diagnostic = Some(diagnostic.to_string());
        self.state = ChildState::Failed(result);
    }

    pub(super) fn record_paths(&self) -> usize {
        match &self.state {
            ChildState::Running => 0,
            ChildState::Completed(result)
            | ChildState::Aborted(result)
            | ChildState::Failed(result)
            | ChildState::Interrupted(result) => result.changed_paths_total,
        }
    }
}

pub(super) fn result_mut(state: &mut ChildState) -> Option<&mut ChildResult> {
    match state {
        ChildState::Running => None,
        ChildState::Completed(result)
        | ChildState::Aborted(result)
        | ChildState::Failed(result)
        | ChildState::Interrupted(result) => Some(result),
    }
}

pub(super) fn task_args(call: &ToolCall) -> Result<Args> {
    let mut args: Args = serde_json::from_str(&call.function.arguments)
        .map_err(|error| Error::Tool(format!("task arguments are invalid: {error}")))?;
    if let Some(id) = &args.child {
        if id.trim().is_empty() {
            return Err(Error::Tool("task child id is empty".to_string()));
        }
        if !args.assignment.trim().is_empty() {
            return Err(Error::Tool(
                "task takes either child or assignment, not both".to_string(),
            ));
        }
        return Ok(args);
    }
    if args.kill {
        return Err(Error::Tool("task kill requires child".to_string()));
    }
    let assignment = args.assignment.trim();
    if assignment.is_empty() {
        return Err(Error::Tool("task requires assignment".to_string()));
    }
    args.assignment = assignment.to_string();
    Ok(args)
}

pub(super) fn rejected(count: usize, message: &str) -> Vec<Result<Execution>> {
    (0..count)
        .map(|_| Err(Error::Denied(message.to_string())))
        .collect()
}
