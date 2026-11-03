use std::future::Future;
use std::task::{Context, Poll, Waker};

use super::*;
use tokio::task::JoinError;

pub(super) fn poll_ready(
    handle: &mut JoinHandle<Done>,
) -> Option<std::result::Result<Done, JoinError>> {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match std::pin::Pin::new(handle).poll(&mut context) {
        Poll::Ready(joined) => Some(joined),
        Poll::Pending => None,
    }
}

pub(super) async fn killed(mut spawned: Spawned, parent: &Path, reason: &str) -> Result<Done> {
    spawned.handle.abort();
    if let Ok(done) = (&mut spawned.handle).await {
        return Ok(done);
    }
    aborted(spawned, parent, reason)
}

fn forgotten(mut spawned: Spawned, parent: &Path, reason: &str) -> Result<Done> {
    spawned.handle.abort();
    match poll_ready(&mut spawned.handle) {
        Some(Ok(done)) => Ok(done),
        Some(Err(_)) | None => aborted(spawned, parent, reason),
    }
}

fn aborted(spawned: Spawned, parent: &Path, reason: &str) -> Result<Done> {
    let mut record = spawned.record;
    let mut diagnostic = reason.to_string();
    let stopped = crate::tools::command::stop_jobs(&record.workspace, &record.id.to_string())?;
    if !stopped.is_empty() {
        diagnostic.push('\n');
        diagnostic.push_str(&stopped.join("\n"));
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

impl<P: Provider> Loop<P> {
    pub(in crate::runtime) fn forget_spawned(
        &mut self,
        scope: crate::child::ChildScope,
        reason: &str,
    ) -> Result<()> {
        let ids: Vec<String> = self
            .spawned
            .iter()
            .filter(|(_, spawned)| {
                scope == crate::child::ChildScope::All || !spawned.record.background
            })
            .map(|(id, _)| id.clone())
            .collect();
        let mut settled = Ok(());
        for id in &ids {
            let Some(spawned) = self.spawned.remove(id) else {
                continue;
            };
            let done = forgotten(spawned, &self.workspace, reason);
            if let Err(error) = done.and_then(|done| self.task_settle(done)) {
                settled = settled.and(Err(error));
            }
        }
        self.discard_events(&ids);
        settled
    }
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
