use super::*;
use summonr_agent::tools::Builtin;

impl Model {
    pub(crate) fn set_busy(&mut self, activity: Option<Activity>) {
        self.busy_since = activity.map(|activity| (activity, Instant::now()));
        if activity == Some(Activity::Turn) {
            self.doing = Some(Doing::Thinking);
            self.speed.start(Instant::now());
        }
        if activity.is_some() {
            self.rounds = 0;
        } else {
            self.doing = None;
        }
    }

    pub(crate) fn receipt(&mut self, text: &str) {
        let receipt = match self.entries.last() {
            Some(Entry::Assistant { text: body, .. }) => {
                text.strip_prefix(body).map(str::trim_start).unwrap_or(text)
            }
            _ => text,
        };
        if !receipt.is_empty() {
            self.trail(Entry::Receipt(receipt.to_string()));
        }
    }

    pub(crate) fn trail(&mut self, entry: Entry) {
        let at = self.entries.len().saturating_sub(1);
        let answer = matches!(self.entries.last(), Some(Entry::Assistant { .. }));
        if answer && at >= self.committed {
            self.entries.insert(at, entry);
        } else {
            self.entries.push(entry);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.clear_transcript();
        self.entries
            .push(Entry::System("history cleared".to_string()));
    }

    pub(super) fn clear_transcript(&mut self) {
        self.committed = 0;
        self.entries.clear();
        self.tools.clear();
        self.agents.clear();
        self.ledger.clear();
        self.child_todos.clear();
        self.plan_current = None;
        self.plan_total = 0;
        self.plan_objective.clear();
    }

    pub(crate) fn apply_event(&mut self, event: Event) {
        match event {
            Event::Reasoning { .. } | Event::Delta { .. } => self.apply_stream(event),
            Event::ToolCall { .. } | Event::ToolResult { .. } | Event::Progress { .. } => {
                self.apply_tool(event);
            }
            Event::Assistant { .. }
            | Event::Aborted { .. }
            | Event::Merged { .. }
            | Event::Mail { .. }
            | Event::Steered { .. }
            | Event::Note { .. }
            | Event::Verify { .. }
            | Event::Receipt { .. } => self.apply_entry(event),
            Event::Work { progress } => {
                self.ledger = progress
                    .ledger
                    .iter()
                    .map(String::as_str)
                    .filter_map(LedgerRow::parse)
                    .chain(
                        progress
                            .todos
                            .iter()
                            .map(|todo| LedgerRow::todo(todo, 0, None)),
                    )
                    .chain(self.child_todos.iter().flat_map(|(_, lines)| lines.clone()))
                    .collect();
                self.plan_current = progress.current;
                self.plan_total = progress.total;
                self.plan_objective = progress.objective.clone();
                if progress.reason.is_some() {
                    self.entries.push(Entry::System(progress.to_string()));
                }
            }
            Event::Usage {
                prompt_tokens,
                completion_tokens,
                reasoning_tokens,
                cached_tokens,
                cache_write_tokens: _,
                millis: _,
                prefix_hash,
                cost,
                side,
            } => self.record_usage(summonr_agent::hub::Round {
                prompt: prompt_tokens,
                completion: completion_tokens,
                reasoning: reasoning_tokens,
                cached: cached_tokens,
                prefix_hash,
                cost,
                side,
            }),
            Event::Approval { .. }
            | Event::Elicit { .. }
            | Event::Spawned { .. }
            | Event::Dismissed
            | Event::Session { .. }
            | Event::Step { .. } => {}
            Event::Unreported { tokens } => self.unreported_tokens = tokens,
            Event::Child { id, event } => self.apply_child(&id, *event),
        }
    }

    fn apply_stream(&mut self, event: Event) {
        match event {
            Event::Reasoning { text } => {
                self.speed.delta(text.chars().count(), Instant::now());
                self.doing = Some(Doing::Thinking);
                match self.entries.last_mut() {
                    Some(Entry::Thinking(body)) => body.push_str(&text),
                    _ => self.entries.push(Entry::Thinking(text)),
                }
            }
            Event::Delta { text } => {
                self.speed.delta(text.chars().count(), Instant::now());
                self.doing = None;
                match self.entries.last_mut() {
                    Some(Entry::Assistant {
                        text: body,
                        done: false,
                    }) => body.push_str(&text),
                    _ => self.entries.push(Entry::Assistant { text, done: false }),
                }
            }
            _other => {}
        }
    }

    fn apply_tool(&mut self, event: Event) {
        match event {
            Event::ToolCall {
                id,
                name,
                arguments,
            } => {
                let shown = match (Builtin::of(&name), summonr_agent::tools::retired_op(&name)) {
                    (Some(Builtin::Read), Some("image")) => Some((arguments.clone(), false)),
                    (Some(Builtin::Read), None | Some("file")) => Some((arguments.clone(), true)),
                    _ => None,
                };
                self.start_tool(id, name, arguments);
                if let Some((arguments, quiet)) = shown {
                    self.show_call(&arguments, quiet);
                }
            }
            Event::ToolResult {
                id,
                success,
                output,
                patch,
                ..
            } => {
                self.finish_tool(&id, success, output, patch);
            }
            Event::Progress {
                progress,
                total,
                message,
                ..
            } => {
                if let Some(Doing::Tool { arg, .. }) = self.doing.as_mut() {
                    *arg = summonr_agent::tools::progress_text(progress, total, message.as_deref());
                }
            }
            _other => {}
        }
    }

    fn apply_entry(&mut self, event: Event) {
        match event {
            Event::Assistant { text } => self.finish_assistant(text),
            Event::Aborted {
                code: summonr_agent::tools::AbortCode::Cancelled,
                ..
            } => self.entries.push(Entry::Aborted(
                "cancelled · F5 or /retry continues · Up edits the prompt".into(),
            )),
            Event::Aborted { reason, .. } => self.entries.push(Entry::Aborted(reason)),
            Event::Merged {
                branch,
                base,
                files,
            } => self.entries.push(Entry::System(format!(
                "merged {branch} into {base}: {} file{}",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            ))),
            Event::Mail { from, text } => self.entries.push(Entry::Mail { from, text }),
            Event::Steered { text } => self.entries.push(Entry::User(text)),
            Event::Note { text } => self.entries.push(Entry::System(text)),
            Event::Verify { command, passed } => self.trail(Entry::Verify { command, passed }),
            Event::Receipt { receipt } => self.trail(Entry::Receipt(receipt.text)),
            _other => {}
        }
    }

    pub(crate) fn child_todos(&mut self, tag: &str, todos: &[summonr_agent::tools::Todo]) {
        let shown: usize = self.child_todos.iter().map(|(_, lines)| lines.len()).sum();
        self.ledger
            .truncate(self.ledger.len().saturating_sub(shown));
        self.child_todos.retain(|(child, _)| child != tag);
        if !todos.is_empty() {
            let lines = todos
                .iter()
                .map(|todo| LedgerRow::todo(todo, 2, Some(tag)))
                .collect();
            self.child_todos.push((tag.to_string(), lines));
        }
        self.ledger
            .extend(self.child_todos.iter().flat_map(|(_, lines)| lines.clone()));
    }

    pub(crate) fn record_usage(&mut self, round: Round) {
        self.rounds += 1;
        self.speed
            .finish(round.completion.max(round.reasoning), Instant::now());
        self.usage.push(round);
    }
}
