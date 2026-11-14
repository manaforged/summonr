use serde::{Deserialize, Serialize};

/// One session event, shared by the journal projection, the hub, and the views.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        id: String,
        success: bool,
        #[serde(default)]
        mutated: bool,
        output: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        millis: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        patch: Option<String>,
    },
    Session {
        id: String,
    },
    Assistant {
        text: String,
    },
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
        #[serde(default)]
        reasoning_tokens: u32,
        cached_tokens: u32,
        #[serde(default)]
        cache_write_tokens: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        millis: Option<u64>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        prefix_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost: Option<f64>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        side: bool,
    },
    Spawned {
        assignment: String,
        access: crate::Access,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        call: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        background: bool,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        workspace: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        agent: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        name: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        model: String,
    },
    Unreported {
        tokens: u64,
    },
    Mail {
        from: String,
        text: String,
    },
    Steered {
        text: String,
    },
    /// A user message a view shows with its images; never serialized.
    #[serde(skip)]
    User {
        text: String,
        images: Vec<crate::Image>,
    },
    /// A harness message the model received as user text; never serialized.
    #[serde(skip)]
    Injected {
        text: String,
    },
    /// The conversation history was cleared; never serialized.
    #[serde(skip)]
    Cleared,
    Aborted {
        reason: String,
        #[serde(default)]
        code: AbortCode,
    },
    Merged {
        branch: String,
        base: String,
        files: Vec<String>,
    },
    Delta {
        text: String,
    },
    Reasoning {
        text: String,
    },
    Note {
        text: String,
    },
    Progress {
        id: String,
        progress: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        total: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Elicit {
        form: Option<crate::Form>,
        #[serde(default)]
        dialog: u64,
    },
    Approval {
        detail: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        persist: bool,
        #[serde(default)]
        dialog: u64,
    },
    Verify {
        command: String,
        passed: bool,
    },
    Receipt {
        receipt: crate::Receipt,
    },
    Work {
        progress: crate::WorkProgress,
    },
    Step {
        index: usize,
        total: usize,
        state: StepState,
        check: Option<String>,
    },
    Child {
        id: String,
        event: Box<Event>,
    },
    /// Hides a child card without changing its session or history.
    Dismissed,
}

/// Why a child checkout was kept on disk.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    Conflict,
    OffBase,
    Unreadable,
    Unremoved,
}

impl Hold {
    /// The reason as a phrase for messages.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Conflict => "conflicts with the parent",
            Self::OffBase => "moved off the parent base",
            Self::Unreadable => "could not be read",
            Self::Unremoved => "landed, but could not be removed",
        }
    }
}

/// A child checkout kept on disk and why.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Kept {
    pub checkout: std::path::PathBuf,
    pub hold: Hold,
}

/// Why a turn or a run stopped early.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AbortCode {
    TimeBudget,
    TokenBudget,
    Interrupted,
    Cancelled,
    #[default]
    Error,
}

impl AbortCode {
    /// The child agents this stop also stops.
    #[must_use]
    pub fn child_scope(self) -> crate::ChildScope {
        if matches!(self, Self::Cancelled | Self::Error) {
            crate::ChildScope::Foreground
        } else {
            crate::ChildScope::All
        }
    }
}

/// An early stop with its code and reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Abort {
    pub code: AbortCode,
    pub reason: String,
}

impl Abort {
    /// The stop for a reached work-time limit.
    #[must_use]
    pub fn time_limit() -> Self {
        Self {
            code: AbortCode::TimeBudget,
            reason: "work-time limit reached".to_string(),
        }
    }
}

/// A progress figure as `progress`, `progress/total`, and an optional message.
#[must_use]
pub fn progress_text(progress: f64, total: Option<f64>, message: Option<&str>) -> String {
    let amount = total.map_or_else(
        || format!("{progress}"),
        |total| format!("{progress}/{total}"),
    );
    message.map_or_else(|| amount.clone(), |message| format!("{amount} · {message}"))
}

/// The state of one plan step in a step-mode run.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepState {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "check passed")]
    CheckPassed,
    #[serde(rename = "check failed")]
    CheckFailed,
    #[serde(rename = "done")]
    Done,
}

impl StepState {
    /// The state as the text the JSON event carries.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::CheckPassed => "check passed",
            Self::CheckFailed => "check failed",
            Self::Done => "done",
        }
    }
}

#[cfg(test)]
#[path = "event_tests.rs"]
mod tests;
