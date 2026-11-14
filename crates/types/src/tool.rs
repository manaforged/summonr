use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

use crate::{Image, ToolKind, Usage};

/// A function tool definition.
pub fn tool(name: &str, description: &str, parameters: Value) -> ToolDefinition {
    ToolDefinition {
        kind: ToolKind::Function,
        function: FunctionDefinition {
            name: name.to_string(),
            description: description.to_string(),
            parameters: raw(&object_root(parameters)),
        },
    }
}

fn object_root(parameters: Value) -> Value {
    let Value::Object(mut root) = parameters else {
        return serde_json::json!({"type": "object"});
    };
    let mut names = required(&Value::Object(root.clone()));
    for key in ["anyOf", "oneOf", "allOf"] {
        let Some(Value::Array(branches)) = root.remove(key) else {
            continue;
        };
        let mut shared: Option<BTreeSet<String>> = None;
        for branch in &branches {
            let wanted = required(branch);
            shared = Some(match (key, shared) {
                ("allOf", Some(all)) => all.union(&wanted).cloned().collect(),
                (_, Some(all)) => all.intersection(&wanted).cloned().collect(),
                (_, None) => wanted,
            });
            let Some(Value::Object(fields)) = branch.get("properties") else {
                continue;
            };
            if let Value::Object(properties) = root
                .entry("properties")
                .or_insert_with(|| Value::Object(Map::new()))
            {
                for (name, schema) in fields {
                    properties
                        .entry(name.clone())
                        .or_insert_with(|| schema.clone());
                }
            }
        }
        names.extend(shared.unwrap_or_default());
    }
    if names.is_empty() {
        root.remove("required");
    } else {
        root.insert(
            "required".to_string(),
            Value::Array(names.into_iter().map(Value::String).collect()),
        );
    }
    root.insert("type".to_string(), Value::String("object".to_string()));
    Value::Object(root)
}

fn raw(schema: &Value) -> Box<RawValue> {
    serde_json::value::to_raw_value(schema).unwrap_or_default()
}

fn required(schema: &Value) -> BTreeSet<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// A tool definition sent to a provider.
#[derive(Clone, Debug, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: ToolKind,
    pub function: FunctionDefinition,
}

/// The name, description, and JSON schema of a function tool.
#[derive(Clone, Debug, Serialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Box<RawValue>,
}

impl FunctionDefinition {
    /// The parameters schema as a JSON value.
    #[must_use]
    pub fn schema(&self) -> Value {
        serde_json::from_str(self.parameters.get()).unwrap_or_default()
    }

    /// Replaces the parameters schema with `schema`.
    pub fn set_schema(&mut self, schema: &Value) {
        self.parameters = raw(schema);
    }
}

/// The approval a tool call needs before it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Grant {
    Write(std::path::PathBuf),
    Guarded(std::path::PathBuf),
    Danger { command: String, reason: String },
}

impl Grant {
    /// The approval as the text the prompt shows.
    #[must_use]
    pub fn prompt(&self) -> String {
        match self {
            Self::Write(path) => path.display().to_string(),
            Self::Guarded(path) => format!("protected path {}", path.display()),
            Self::Danger { command, reason } => {
                format!(
                    "{command} — {}",
                    reason.strip_prefix("it ").unwrap_or(reason)
                )
            }
        }
    }
}

/// The result of a tool call.
pub struct ToolOutput {
    pub text: String,
    pub success: bool,
    pub mutated: bool,
    pub grant: Option<Grant>,
    pub images: Vec<Image>,
    pub patch: Option<String>,
}

impl ToolOutput {
    /// A successful result that changed nothing.
    pub fn ok(text: String) -> Self {
        Self {
            text,
            success: true,
            mutated: false,
            grant: None,
            images: Vec::new(),
            patch: None,
        }
    }

    /// A successful result that changed files, with its patch.
    pub fn mutated(text: String, patch: Option<String>) -> Self {
        Self {
            mutated: true,
            patch,
            ..Self::ok(text)
        }
    }

    /// This result with `success` set.
    #[must_use]
    pub fn with_success(mut self, success: bool) -> Self {
        self.success = success;
        self
    }

    /// This result with `mutated` set.
    #[must_use]
    pub fn with_mutated(mut self, mutated: bool) -> Self {
        self.mutated = mutated;
        self
    }
}

/// The text, reasoning, and usage streamed so far.
#[derive(Default)]
pub struct Partial {
    pub text: String,
    pub reasoning: String,
    pub usage: Option<Usage>,
}
