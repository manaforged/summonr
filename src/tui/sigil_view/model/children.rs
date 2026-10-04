use super::*;
use crate::tools::Builtin;

const TITLE_CHARS: usize = 72;

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
            } => self.child_spawn(Agent {
                call: call.clone(),
                background: *background,
                title: title(assignment),
                assignment: assignment.clone(),
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

    fn child_spawn(&mut self, agent: Agent) {
        self.agents.push(agent);
        self.agents_hidden = false;
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
                self.agents_hidden = false;
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
                self.agents_hidden = false;
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

fn title(assignment: &str) -> String {
    let line = assignment.lines().next().unwrap_or_default().trim();
    let sentence = line
        .match_indices(". ")
        .next()
        .map_or(line, |(at, _)| &line[..at]);
    crate::text::ellipsize(sentence.trim_end_matches('.'), TITLE_CHARS).to_string()
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
        } => (compact.text(output).to_string(), false, true),
        Event::Assistant { text } => (compact.text(text).to_string(), true, false),
        Event::Aborted { reason } => (reason.clone(), true, true),
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
            !passed,
        ),
        _ => return None,
    };
    Some(line)
}
