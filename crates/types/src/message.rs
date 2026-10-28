use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use summonr_core::error::{Error, Result};

use crate::Usage;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Image {
    pub media_type: String,
    pub data: String,
}

impl Image {
    pub fn encode(media_type: impl Into<String>, bytes: &[u8]) -> Self {
        Self {
            media_type: media_type.into(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }

    pub fn bytes(&self) -> Result<Vec<u8>> {
        base64::engine::general_purpose::STANDARD
            .decode(&self.data)
            .map_err(|error| Error::Config(format!("cannot decode stored image: {error}")))
    }

    pub fn url(&self) -> String {
        format!("data:{};base64,{}", self.media_type, self.data)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserInput {
    pub text: String,
    pub images: Vec<Image>,
    pub source: Option<Source>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub text: String,
    pub images: usize,
}

impl From<&str> for UserInput {
    fn from(text: &str) -> Self {
        Self {
            text: text.to_string(),
            images: Vec::new(),
            source: None,
        }
    }
}

impl From<String> for UserInput {
    fn from(text: String) -> Self {
        Self {
            text,
            images: Vec::new(),
            source: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing)]
    pub images: Vec<Image>,
    #[serde(
        default,
        deserialize_with = "vec_or_null",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub tool_calls: Vec<ToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(
        default,
        rename = "reasoning_content",
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning: Option<String>,
    #[serde(skip)]
    pub failed: bool,
    #[serde(skip)]
    pub continuation: Option<Continuation>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            images: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning: None,
            failed: false,
            continuation: None,
        }
    }

    pub fn user(input: impl Into<UserInput>) -> Self {
        let input = input.into();
        Self {
            role: Role::User,
            content: input.text,
            images: input.images,
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning: None,
            failed: false,
            continuation: None,
        }
    }

    pub fn tool(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            images: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.into()),
            reasoning: None,
            failed: false,
            continuation: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    #[default]
    Function,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: ToolKind,
    pub function: ToolFunction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<Continuation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum Continuation {
    Anthropic {
        model: String,
        blocks: Vec<Value>,
    },
    Gemini {
        model: String,
        signatures: Vec<String>,
    },
    Responses {
        model: String,
        signatures: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        commentary: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Checked {
    #[default]
    Unchecked,
    Valid,
    Invalid,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
    #[serde(skip)]
    pub checked: Checked,
}

impl PartialEq for ToolFunction {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.arguments == other.arguments
    }
}

impl Eq for ToolFunction {}

impl ToolFunction {
    pub fn check(&mut self) {
        self.checked = if parses(&self.arguments) {
            Checked::Valid
        } else {
            Checked::Invalid
        };
    }

    pub fn valid(&self) -> bool {
        match self.checked {
            Checked::Valid => true,
            Checked::Invalid => false,
            Checked::Unchecked => parses(&self.arguments),
        }
    }
}

fn parses(arguments: &str) -> bool {
    serde_json::from_str::<Value>(arguments).is_ok()
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct AssistantMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default, deserialize_with = "vec_or_null")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, alias = "reasoning", deserialize_with = "reasoning_field")]
    pub reasoning_content: Option<String>,
    #[serde(default, skip_serializing)]
    pub usage: Usage,
    #[serde(default, skip_serializing)]
    pub stop_reason: StopReason,
    #[serde(skip)]
    pub continuation: Option<Continuation>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    #[default]
    Stop,
    ToolCalls,
    Length,
    #[serde(other)]
    Other,
}

impl AssistantMessage {
    pub fn into_message(self) -> Message {
        Message {
            role: Role::Assistant,
            content: self.content.unwrap_or_default(),
            images: Vec::new(),
            tool_calls: self.tool_calls,
            tool_call_id: None,
            reasoning: self.reasoning_content.filter(|text| !text.is_empty()),
            failed: false,
            continuation: self.continuation,
        }
    }
}

pub fn vec_or_null<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

pub fn reasoning_field<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(from_json(value.unwrap_or(Value::Null)))
}

fn from_json(value: Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => empty_to_none(text),
        Value::Object(map) => map
            .get("content")
            .or_else(|| map.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .and_then(empty_to_none),
        Value::Array(parts) => {
            let mut out = String::new();
            for part in parts {
                if let Some(text) = part.as_str() {
                    out.push_str(text);
                } else if let Some(text) = part.get("text").and_then(Value::as_str) {
                    out.push_str(text);
                } else if let Some(text) = part.get("content").and_then(Value::as_str) {
                    out.push_str(text);
                }
            }
            empty_to_none(out)
        }
        _ => None,
    }
}

fn empty_to_none(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}
