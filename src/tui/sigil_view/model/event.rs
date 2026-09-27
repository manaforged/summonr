use super::*;
use crate::tools::Builtin;

impl Model {
    pub(in crate::tui::sigil_view) fn set_busy(&mut self, activity: Option<Activity>) {
        self.busy_since = activity.map(|activity| (activity, Instant::now()));
        if activity == Some(Activity::Turn) {
            self.live_tool = Some(("thinking".to_string(), String::new()));
            self.speed.start(Instant::now());
        }
        if activity.is_some() {
            self.rounds = 0;
        } else {
            self.live_tool = None;
            self.agents.retain(|agent| !agent.done);
        }
    }

    pub(in crate::tui::sigil_view) fn receipt(&mut self, text: &str) {
        let receipt = match self.entries.last() {
            Some(Entry::Assistant { text: body, .. }) => {
                text.strip_prefix(body).map(str::trim_start).unwrap_or(text)
            }
            _ => text,
        };
        if !receipt.is_empty() {
            self.entries.push(Entry::Receipt(receipt.to_string()));
        }
    }

    pub(in crate::tui::sigil_view) fn clear(&mut self) {
        self.entries.clear();
        self.tools.clear();
        self.agents.clear();
        self.ledger.clear();
        self.plan_current = None;
        self.plan_total = 0;
        self.plan_objective.clear();
        self.entries
            .push(Entry::System("history cleared".to_string()));
    }

    pub(in crate::tui::sigil_view) fn apply_event(&mut self, event: Event) {
        match event {
            Event::Reasoning { text } => {
                self.speed.delta(text.chars().count(), Instant::now());
                self.live_tool = Some(("thinking".to_string(), String::new()));
                match self.entries.last_mut() {
                    Some(Entry::Thinking(body)) => body.push_str(&text),
                    _ => self.entries.push(Entry::Thinking(text)),
                }
            }
            Event::Delta { text } => {
                self.speed.delta(text.chars().count(), Instant::now());
                self.live_tool = None;
                match self.entries.last_mut() {
                    Some(Entry::Assistant {
                        text: body,
                        done: false,
                    }) => body.push_str(&text),
                    _ => self.entries.push(Entry::Assistant { text, done: false }),
                }
            }
            Event::ToolCall {
                id,
                name,
                arguments,
            } => {
                let shown = match (Builtin::of(&name), crate::tools::retired_op(&name)) {
                    (Some(Builtin::Read), Some("image")) => Some((arguments.clone(), false)),
                    (Some(Builtin::Read), None | Some("file")) => Some((arguments.clone(), true)),
                    _ => None,
                };
                if Builtin::is(&name, Builtin::Todo) {
                    self.apply_todos(&arguments, None);
                }
                self.start_tool(id, name, arguments);
                if let Some((arguments, quiet)) = shown {
                    self.show_call(&arguments, quiet);
                }
            }
            Event::ToolResult {
                id,
                success,
                output,
                ..
            } => {
                self.finish_tool(&id, success, output);
            }
            Event::Assistant { text } => self.finish_assistant(text),
            Event::Aborted { reason } if reason == "cancelled by user" => self.entries.push(
                Entry::Aborted("cancelled · F5 or /retry continues · Up edits the prompt".into()),
            ),
            Event::Aborted { reason } => self.entries.push(Entry::Aborted(reason)),
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
            Event::Note { text } => {
                if text.starts_with("stream failed — retrying once")
                    && let Some(at) = self
                        .entries
                        .iter()
                        .rposition(|entry| matches!(entry, Entry::Assistant { done: false, .. }))
                {
                    self.entries.invalidate();
                    self.entries.inner.remove(at);
                }
                self.entries.push(Entry::System(text));
            }
            Event::Verify { command, passed } => {
                self.entries.push(Entry::Verify { command, passed });
            }
            Event::Receipt { receipt } => self.entries.push(Entry::Receipt(receipt.text)),
            Event::Work { progress } => {
                self.ledger = progress
                    .ledger
                    .iter()
                    .filter(|line| itemized(line))
                    .cloned()
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
                millis: _,
                prefix_hash,
                cost,
            } => self.record_usage(
                prompt_tokens,
                completion_tokens,
                reasoning_tokens,
                cached_tokens,
                prefix_hash,
                cost,
            ),
            Event::Approval { .. } | Event::Spawned { .. } => {}
            Event::Unreported { tokens } => self.unreported_tokens = tokens,
            Event::Child { id, event } => self.apply_child(&id, *event),
        }
    }

    pub(in crate::tui::sigil_view) fn apply_todos(&mut self, arguments: &str, child: Option<&str>) {
        let Ok(items) = crate::tools::todos_from_args(arguments) else {
            return;
        };
        if items.is_empty() && child.is_none() {
            self.ledger.clear();
            return;
        }
        let lines: Vec<String> = items
            .iter()
            .map(|todo| match child {
                Some(tag) => format!("  {} {tag} · {}", todo.mark(), todo.text),
                None => todo.ledger_line(),
            })
            .collect();
        if let Some(tag) = child {
            let needle = format!(" {tag} · ");
            self.ledger.retain(|line| !line.contains(&needle));
            self.ledger.extend(lines);
        } else {
            self.ledger = lines;
        }
    }

    pub(in crate::tui::sigil_view) fn apply_child(&mut self, id: &str, event: Event) {
        if let Event::ToolCall {
            name, arguments, ..
        } = &event
            && Builtin::is(name, Builtin::Todo)
        {
            self.apply_todos(arguments, Some(&crate::child::tag(id)));
        }
        let tag = crate::child::tag(id);
        if let Event::Spawned { assignment, access } = &event {
            self.agents.push(Agent {
                id: tag,
                assignment: assignment.clone(),
                access: *access,
                started: Instant::now(),
                secs: None,
                last: String::new(),
                done: false,
                failed: false,
                tokens: 0,
                cost: 0.0,
            });
            self.agents_hidden = false;
            return;
        }
        if let Event::Usage {
            prompt_tokens,
            completion_tokens,
            reasoning_tokens,
            cost,
            ..
        } = &event
        {
            let index = match self.agents.iter().position(|agent| agent.id == tag) {
                Some(index) => index,
                None => {
                    self.agents_hidden = false;
                    self.agents.push(Agent::placeholder(tag.clone()));
                    self.agents.len() - 1
                }
            };
            let agent = &mut self.agents[index];
            agent.tokens += u64::from(*prompt_tokens)
                + u64::from(*completion_tokens)
                + u64::from(*reasoning_tokens);
            agent.cost += cost.unwrap_or(0.0);
            return;
        }
        let compact = ToolOutput::Compact;
        let (last, done, failed) = match event {
            Event::ToolCall {
                name, arguments, ..
            } => (
                format!("{name} {}", tool_arg(&arguments))
                    .trim()
                    .to_string(),
                false,
                false,
            ),
            Event::ToolResult {
                success: false,
                output,
                ..
            } => (format!("✗ {}", compact.text(&output)), false, true),
            Event::Assistant { text } => (compact.text(&text).to_string(), true, false),
            Event::Aborted { reason } => (reason, true, true),
            Event::Note { text } => (text, false, false),
            Event::Verify { command, passed } => (
                format!("{} {command}", if passed { "✓" } else { "✗" }),
                false,
                !passed,
            ),
            _ => return,
        };
        if let Some(agent) = self.agents.iter_mut().find(|agent| agent.id == tag) {
            agent.last = last.clone();
            if done && !agent.done {
                agent.secs = Some(agent.started.elapsed().as_secs());
            }
            agent.done = done;
            agent.failed |= failed;
        } else {
            let mut agent = Agent::placeholder(tag.clone());
            agent.last = last.clone();
            agent.done = done;
            agent.failed = failed;
            if done {
                agent.secs = Some(0);
            }
            self.agents.push(agent);
            self.agents_hidden = false;
        }
        if done {
            let failed = self
                .agents
                .iter()
                .find(|agent| agent.id == tag)
                .is_some_and(|agent| agent.failed);
            self.entries.push(Entry::Tool {
                name: "agent".to_string(),
                arg: tag,
                state: if failed {
                    ToolState::Failed
                } else {
                    ToolState::Ok
                },
                output: last,
            });
        }
    }

    pub(in crate::tui::sigil_view) fn record_usage(
        &mut self,
        prompt: u32,
        completion: u32,
        reasoning: u32,
        cached: u32,
        prefix_hash: String,
        cost: Option<f64>,
    ) {
        self.rounds += 1;
        self.speed.finish(completion.max(reasoning), Instant::now());
        self.usage.push(Round {
            prompt,
            completion,
            reasoning,
            cached,
            prefix_hash,
            cost,
        });
        self.prompt_tokens += u64::from(prompt);
        self.completion_tokens += u64::from(completion);
        self.reasoning_tokens += u64::from(reasoning);
        self.cached_tokens += u64::from(cached);
    }
}
