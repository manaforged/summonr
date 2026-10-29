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
    log::records(path, 0)
        .try_for_each(|line| line.map(|line| state.apply(line)))
        .map_err(|error| Error::Config(format!("cannot read child session log: {error}")))?;
    let usage = Usage {
        prompt_tokens: Usage::clamp(state.metered.input),
        completion_tokens: Usage::clamp(state.metered.output),
        cached_tokens: Usage::clamp(state.metered.cached),
        ..Usage::default()
    };
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
        kept: None,
    }
}

impl ChildRecord {
    pub(super) fn note_record(&mut self, diagnostic: &str) {
        if let Some(result) = self.state.result_mut() {
            result.diagnostic = Some(diagnostic.to_string());
        }
    }

    pub(super) fn fail_record(&mut self, diagnostic: &str) {
        let Some(result) = self.state.result_mut() else {
            return;
        };
        let mut result = std::mem::replace(result, empty(None));
        result.diagnostic = Some(diagnostic.to_string());
        self.state = ChildState::Failed(result);
    }

    pub(super) fn record_paths(&self) -> usize {
        self.state
            .result()
            .map_or(0, |result| result.changed_paths_total)
    }
}

impl ChildState {
    pub(crate) fn result(&self) -> Option<&ChildResult> {
        match self {
            Self::Running => None,
            Self::Completed(result)
            | Self::Aborted(result)
            | Self::Failed(result)
            | Self::Interrupted(result) => Some(result),
        }
    }

    pub(crate) fn result_mut(&mut self) -> Option<&mut ChildResult> {
        match self {
            Self::Running => None,
            Self::Completed(result)
            | Self::Aborted(result)
            | Self::Failed(result)
            | Self::Interrupted(result) => Some(result),
        }
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
