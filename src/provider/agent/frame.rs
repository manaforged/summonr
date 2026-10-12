use serde::Deserialize;
use serde_json::Value;

use super::super::wire::anthropic::Counted;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Frame {
    ControlRequest {
        request_id: String,
        request: Control,
    },
    ControlResponse {
        response: Reply,
    },
    StreamEvent {
        event: Streamed,
        #[serde(default)]
        parent_tool_use_id: Option<String>,
    },
    Assistant {
        message: Said,
        #[serde(default)]
        parent_tool_use_id: Option<String>,
    },
    RateLimitEvent {
        rate_limit_info: RateLimit,
    },
    Result(Box<Finished>),
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "subtype", rename_all = "snake_case")]
pub(super) enum Control {
    McpMessage {
        message: Value,
    },
    CanUseTool {
        #[serde(default)]
        input: Value,
        #[serde(default)]
        mcp_server: Option<Origin>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub(super) struct Origin {
    pub(super) name: String,
}

#[derive(Deserialize)]
#[serde(tag = "method")]
pub(super) enum Rpc {
    #[serde(rename = "initialize")]
    Initialize {
        id: Value,
        #[serde(default)]
        params: Value,
    },
    #[serde(rename = "tools/list")]
    ToolsList { id: Value },
    #[serde(rename = "tools/call")]
    ToolsCall { id: Value, params: CallParams },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub(super) struct CallParams {
    pub(super) name: String,
    #[serde(default)]
    pub(super) arguments: Value,
    #[serde(default, rename = "_meta")]
    pub(super) meta: Meta,
}

#[derive(Default, Deserialize)]
pub(super) struct Meta {
    #[serde(default, rename = "claudecode/toolUseId")]
    pub(super) tool_use_id: String,
}

#[derive(Deserialize)]
pub(super) struct Reply {
    pub(super) request_id: String,
    pub(super) subtype: ReplyKind,
    #[serde(default)]
    pub(super) response: Value,
    #[serde(default)]
    pub(super) error: Value,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReplyKind {
    Success,
    Error,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Streamed {
    ContentBlockDelta {
        delta: Piece,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Piece {
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub(super) struct Said {
    #[serde(default)]
    pub(super) id: String,
    #[serde(default)]
    pub(super) content: Vec<Block>,
    #[serde(default)]
    pub(super) usage: Option<Counted>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Block {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub(super) struct RateLimit {
    pub(super) status: LimitStatus,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum LimitStatus {
    Rejected,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub(super) struct Finished {
    #[serde(default)]
    pub(super) is_error: bool,
    #[serde(default)]
    pub(super) subtype: String,
    #[serde(default)]
    pub(super) result: Option<String>,
    #[serde(default)]
    pub(super) errors: Vec<String>,
    #[serde(default)]
    pub(super) usage: Counted,
    #[serde(default)]
    pub(super) api_error_status: Option<u16>,
}

#[derive(Default, Deserialize)]
pub(super) struct Ready {
    #[serde(default)]
    pub(super) models: Vec<Listed>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Listed {
    pub(super) resolved_model: String,
    #[serde(default)]
    pub(super) display_name: Option<String>,
    #[serde(default)]
    pub(super) description: Option<String>,
    #[serde(default)]
    pub(super) supports_effort: bool,
}
