use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

/// The kind of a run: one turn, a step plan, or a goal.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    #[default]
    Single,
    Steps,
    Goal,
}

impl WorkKind {
    /// The kind's display name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Single => "auto",
            Self::Steps => "work",
            Self::Goal => "goal",
        }
    }
}

/// The phase of a run.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPhase {
    Plan,
    Execute,
    Verify,
    Completed,
    Failed,
}

impl Display for WorkPhase {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Plan => "planning",
            Self::Execute => "running",
            Self::Verify => "verifying",
            Self::Completed => "completed",
            Self::Failed => "failed",
        })
    }
}

/// The progress of a run.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct WorkProgress {
    pub kind: WorkKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub objective: String,
    pub phase: WorkPhase,
    #[serde(default)]
    pub checks_failed: bool,
    pub current: Option<usize>,
    pub total: usize,
    pub attempts: u16,
    pub rounds: u32,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ledger: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub todos: Vec<Todo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub touched: Vec<String>,
}

impl WorkProgress {
    /// Whether the run has finished.
    #[must_use]
    pub fn terminal(&self) -> bool {
        matches!(self.phase, WorkPhase::Completed | WorkPhase::Failed)
    }

    /// The exit code of a finished run.
    #[must_use]
    pub fn exit_code(&self) -> Option<i32> {
        match self.phase {
            WorkPhase::Completed => Some(0),
            WorkPhase::Failed if self.checks_failed => Some(2),
            WorkPhase::Failed => Some(3),
            WorkPhase::Plan | WorkPhase::Execute | WorkPhase::Verify => None,
        }
    }

    /// The process exit code: 2 when checks failed, 3 when stopped short, else the run's code.
    #[must_use]
    pub fn exit_status(&self, checks_failed: bool, aborted: bool) -> i32 {
        if checks_failed {
            2
        } else if aborted {
            3
        } else {
            self.exit_code().unwrap_or(0)
        }
    }
}

impl Display for WorkProgress {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match (self.kind, self.current) {
            (WorkKind::Steps, Some(current)) => {
                write!(
                    formatter,
                    "Work step {current}/{} · {}",
                    self.total, self.phase
                )?;
            }
            (WorkKind::Goal, _) => {
                write!(formatter, "Work goal turn {} · {}", self.rounds, self.phase)?;
            }
            _ => write!(formatter, "Work · {}", self.phase)?,
        }
        if let Some(reason) = &self.reason {
            write!(formatter, " · {reason}")?;
        }
        Ok(())
    }
}

/// The status of a todo item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TodoStatus {
    Pending,
    Active,
    Done,
    Blocked,
}

impl TodoStatus {
    /// The status named by `name`, accepting the Agent Client Protocol names.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "pending" => Some(Self::Pending),
            "active" | "in_progress" => Some(Self::Active),
            "done" | "completed" => Some(Self::Done),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }

    /// The status's Agent Client Protocol name.
    pub fn acp(self) -> &'static str {
        match self {
            Self::Pending | Self::Blocked => "pending",
            Self::Active => "in_progress",
            Self::Done => "completed",
        }
    }
}

/// One todo item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    pub text: String,
    pub status: TodoStatus,
}

impl Todo {
    /// The item's status mark.
    pub fn mark(&self) -> &'static str {
        match self.status {
            TodoStatus::Done => "✓",
            TodoStatus::Active => "●",
            TodoStatus::Pending => "○",
            TodoStatus::Blocked => "!",
        }
    }

    /// Whether the item is still to do.
    pub fn open(&self) -> bool {
        matches!(self.status, TodoStatus::Active | TodoStatus::Pending)
    }

    /// The item as its mark and text.
    pub fn line(&self) -> String {
        format!("{} {}", self.mark(), self.text)
    }
}
