use serde::{Deserialize, Serialize};

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        millis: Option<u64>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        prefix_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost: Option<f64>,
    },
    Unreported {
        tokens: u64,
    },
    Mail {
        from: String,
        text: String,
    },
    Aborted {
        reason: String,
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
    Approval {
        detail: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        persist: bool,
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
    Child {
        id: String,
        event: Box<Event>,
    },
}
