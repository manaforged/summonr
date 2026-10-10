use super::*;
use crate::tools::Builtin;

impl Model {
    pub(in crate::tui::sigil_view) fn apply_child(&mut self, id: &str, event: Event) {
        if let Event::ToolCall {
            name, arguments, ..
        } = &event
            && Builtin::is(name, Builtin::Todo)
        {
            self.apply_todos(arguments, Some(&crate::child::tag(id)));
        }
        let tag = crate::child::tag(id);
        match &event {
            Event::Spawned {
                assignment,
                access,
                call,
                background,
                workspace,
                agent,
            } => self.child_spawn(Agent {
                call: call.clone(),
                background: *background,
                title: Agent::title_of(assignment),
                agent: agent.clone(),
                assignment: assignment.clone(),
                workspace: workspace.clone(),
                access: *access,
                ..Agent::placeholder(tag)
            }),
            Event::Usage {
                prompt_tokens,
                completion_tokens,
                reasoning_tokens,
                cost,
                ..
            } => self.child_usage(
                tag,
                *prompt_tokens,
                *completion_tokens,
                *reasoning_tokens,
                *cost,
            ),
            other => {
                let Some((last, done, failed)) = child_line(other) else {
                    return;
                };
                self.child_status(tag, last, done, failed);
            }
        }
        self.entries.invalidate();
    }

    pub(in crate::tui::sigil_view) fn child_receipt(
        &self,
        call: &str,
    ) -> Option<crate::child::ChildRecord> {
        let output = self.entries.iter().rev().find_map(|entry| match entry {
            Entry::Tool { id, output, .. } if id == call => Some(output),
            Entry::ToolGroup { calls, .. } => calls
                .iter()
                .find(|item| item.id == call)
                .map(|item| &item.output),
            _ => None,
        })?;
        serde_json::from_str(output).ok()
    }

    fn child_spawn(&mut self, agent: Agent) {
        self.agents.push(agent);
    }

    fn child_usage(
        &mut self,
        tag: String,
        prompt: u32,
        completion: u32,
        reasoning: u32,
        cost: Option<f64>,
    ) {
        let index = match self.agents.iter().position(|agent| agent.id == tag) {
            Some(index) => index,
            None => {
                self.agents.push(Agent::placeholder(tag));
                self.agents.len() - 1
            }
        };
        let agent = &mut self.agents[index];
        agent.tokens += u64::from(prompt) + u64::from(completion) + u64::from(reasoning);
        agent.cost += cost.unwrap_or(0.0);
    }

    fn child_status(&mut self, tag: String, last: String, done: bool, failed: bool) {
        let index = match self.agents.iter().position(|agent| agent.id == tag) {
            Some(index) => index,
            None => {
                self.agents.push(Agent::placeholder(tag));
                self.agents.len() - 1
            }
        };
        let agent = &mut self.agents[index];
        let finished = done && !agent.done;
        agent.last = last;
        if finished {
            agent.secs = Some(agent.started.elapsed().as_secs());
        }
        agent.done |= done;
        agent.failed |= failed;
        if finished && agent.background {
            let agent = agent.clone();
            self.entries.push(Entry::Tool {
                id: String::new(),
                name: "Background".to_string(),
                arg: format!(
                    "{} · {} · {} tok",
                    agent.title,
                    agent.clock(),
                    crate::provider::compact(agent.tokens)
                ),
                state: agent.state(),
                output: agent.last,
                expanded: false,
                patch: None,
            });
        }
    }
}

fn child_line(event: &Event) -> Option<(String, bool, bool)> {
    let compact = ToolOutput::Compact;
    let line = match event {
        Event::ToolCall {
            name, arguments, ..
        } => (
            format!(
                "{} {}",
                Builtin::of(name).map_or(name.as_str(), |kind| kind.verb()),
                tool_arg(arguments)
            )
            .trim()
            .to_string(),
            false,
            false,
        ),
        Event::ToolResult {
            success: false,
            output,
            ..
        } => (compact.text(output).to_string(), false, false),
        Event::Assistant { text } => (compact.text(text).to_string(), true, false),
        Event::Aborted { reason, .. } => (reason.clone(), true, true),
        Event::Note { text } => (text.clone(), false, false),
        Event::Verify { command, passed } => (
            format!(
                "{} {command}",
                if *passed {
                    "verify passed:"
                } else {
                    "verify failed:"
                }
            ),
            false,
            false,
        ),
        _ => return None,
    };
    Some(line)
}
