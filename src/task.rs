use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::provider::{FunctionDefinition, Message, ToolDefinition, ToolKind};

const MAX_STEPS: usize = 32;
const MAX_TEXT_CHARS: usize = 2000;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TaskStep {
    pub text: String,
    pub status: TaskStatus,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TaskState {
    steps: Vec<TaskStep>,
}

impl TaskState {
    pub(crate) fn transition(current: Option<&Self>, arguments: &str) -> Result<(Self, String)> {
        let request: Request = serde_json::from_str(arguments)
            .map_err(|error| Error::Tool(format!("task_state arguments are invalid: {error}")))?;
        match request.action {
            Action::Start => Self::start(current, request.steps),
            Action::Advance => Self::advance(current, request.steps),
            Action::Revise => Self::revise(current, request.steps),
        }
    }

    #[must_use]
    pub fn steps(&self) -> &[TaskStep] {
        &self.steps
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.steps
            .iter()
            .all(|step| step.status == TaskStatus::Completed)
    }

    pub(crate) fn current(&self) -> Option<&str> {
        self.steps
            .iter()
            .find(|step| step.status == TaskStatus::InProgress)
            .map(|step| step.text.as_str())
    }

    fn start(current: Option<&Self>, steps: Option<Vec<String>>) -> Result<(Self, String)> {
        if current.is_some_and(|state| !state.is_complete()) {
            return Err(Error::Tool(
                "finish the active task before starting another".to_string(),
            ));
        }
        let state = Self::from_text(steps_required(steps, "start")?, 0)?;
        let text = format!("started 1/{}: {}", state.steps.len(), state.steps[0].text);
        Ok((state, text))
    }

    fn advance(current: Option<&Self>, steps: Option<Vec<String>>) -> Result<(Self, String)> {
        if steps.is_some() {
            return Err(Error::Tool("advance does not accept steps".to_string()));
        }
        let mut state = current
            .filter(|state| !state.is_complete())
            .cloned()
            .ok_or_else(|| Error::Tool("there is no active task to advance".to_string()))?;
        let Some(index) = state
            .steps
            .iter()
            .position(|step| step.status == TaskStatus::InProgress)
        else {
            return Err(Error::Tool(
                "the active task has no current step".to_string(),
            ));
        };
        state.steps[index].status = TaskStatus::Completed;
        let total = state.steps.len();
        let text = if let Some(next) = state.steps.get_mut(index + 1) {
            next.status = TaskStatus::InProgress;
            format!("advanced {}/{}: {}", index + 2, total, next.text)
        } else {
            "completed task".to_string()
        };
        Ok((state, text))
    }

    fn revise(current: Option<&Self>, steps: Option<Vec<String>>) -> Result<(Self, String)> {
        let current = current
            .filter(|state| !state.is_complete())
            .ok_or_else(|| Error::Tool("there is no active task to revise".to_string()))?;
        let completed = current
            .steps
            .iter()
            .take_while(|step| step.status == TaskStatus::Completed)
            .count();
        let state = Self::from_text(steps_required(steps, "revise")?, completed)?;
        if state.steps[..completed] != current.steps[..completed] {
            return Err(Error::Tool(
                "revise cannot change completed steps".to_string(),
            ));
        }
        let current = state.current().unwrap_or("the current step").to_string();
        Ok((state, format!("revised task: {current}")))
    }

    fn from_text(steps: Vec<String>, completed: usize) -> Result<Self> {
        if steps.is_empty() || steps.len() > MAX_STEPS {
            return Err(Error::Tool(format!(
                "task_state requires 1-{MAX_STEPS} steps"
            )));
        }
        if completed >= steps.len() {
            return Err(Error::Tool(
                "revise must keep at least one unfinished step".to_string(),
            ));
        }
        let mut normalized = Vec::with_capacity(steps.len());
        for (index, text) in steps.into_iter().enumerate() {
            let text = text.trim().to_string();
            if text.is_empty() || text.chars().count() > MAX_TEXT_CHARS {
                return Err(Error::Tool(format!(
                    "each task step must contain 1-{MAX_TEXT_CHARS} characters"
                )));
            }
            let status = if index < completed {
                TaskStatus::Completed
            } else if index == completed {
                TaskStatus::InProgress
            } else {
                TaskStatus::Pending
            };
            normalized.push(TaskStep { text, status });
        }
        Ok(Self { steps: normalized })
    }
}

pub(crate) fn from_history(messages: &[Message]) -> Option<TaskState> {
    let mut task = None;
    for message in messages {
        for call in &message.tool_calls {
            if call.function.name == "task_state"
                && let Ok((next, _)) =
                    TaskState::transition(task.as_ref(), &call.function.arguments)
            {
                task = Some(next);
            }
        }
    }
    task
}

impl Display for TaskState {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        for (index, step) in self.steps.iter().enumerate() {
            if index > 0 {
                writeln!(formatter)?;
            }
            let mark = match step.status {
                TaskStatus::Pending => " ",
                TaskStatus::InProgress => ">",
                TaskStatus::Completed => "x",
            };
            write!(formatter, "[{mark}] {}. {}", index + 1, step.text)?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct Request {
    action: Action,
    #[serde(default)]
    steps: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Start,
    Advance,
    Revise,
}

fn steps_required(steps: Option<Vec<String>>, action: &str) -> Result<Vec<String>> {
    steps.ok_or_else(|| Error::Tool(format!("{action} requires steps")))
}

pub(crate) fn definition() -> ToolDefinition {
    ToolDefinition {
        kind: ToolKind::Function,
        function: FunctionDefinition {
            name: "task_state".to_string(),
            description: "Track visible progress for an engineering task. Start once with the ordered steps before edit_file or write_file. Put task_state first in the same response as the edit. Call advance after finishing exactly one step, and revise when discoveries change the unfinished work. A final answer is blocked until every active task step is complete.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["start", "advance", "revise"]},
                    "steps": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": MAX_STEPS,
                        "items": {"type": "string", "minLength": 1, "maxLength": MAX_TEXT_CHARS}
                    }
                },
                "required": ["action"],
                "additionalProperties": false
            }),
        },
    }
}

pub(crate) fn guards(name: &str) -> bool {
    matches!(name, "edit_file" | "write_file")
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
