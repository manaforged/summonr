use super::*;
use crate::provider::FunctionDefinition;

const DEFAULT_ROUNDS: usize = 8;
const MAX_ROUNDS: usize = 16;

pub(super) fn definition() -> ToolDefinition {
    ToolDefinition {
        kind: ToolKind::Function,
        function: FunctionDefinition {
            name: "agent".to_string(),
            description: "Run a nested agent on one prompt. Depth 1. Returns its final text."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string"},
                    "max_rounds": {"type": "integer", "minimum": 1, "maximum": 16}
                },
                "required": ["prompt"],
                "additionalProperties": false
            }),
        },
    }
}

impl<P: Provider> Loop<P> {
    pub(super) async fn run_nested(&mut self, arguments: &str) -> Result<crate::tools::ToolOutput> {
        let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
        let prompt = args
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| Error::Tool("agent needs a prompt".to_string()))?;
        let max_rounds = args
            .get("max_rounds")
            .and_then(serde_json::Value::as_u64)
            .map(|n| usize::try_from(n).unwrap_or(DEFAULT_ROUNDS))
            .unwrap_or(DEFAULT_ROUNDS)
            .clamp(1, MAX_ROUNDS);
        let tools = self.tools.definitions();
        let mut messages = vec![
            Message::system(format!(
                "{}\nYou are a nested agent. Do the prompt. You cannot spawn another agent.",
                self.system_prompt
            )),
            Message::user(prompt),
        ];
        let mut last = String::new();
        for _ in 0..max_rounds {
            let request = CompletionRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                tools: tools.clone(),
                max_output_tokens: DEFAULT_ROUND_OUTPUT_TOKENS,
            };
            let assistant = self.provider.complete(&request).await?;
            let message = assistant.into_message();
            last.clone_from(&message.content);
            let calls = message.tool_calls.clone();
            messages.push(message);
            if calls.is_empty() {
                return Ok(crate::tools::ToolOutput {
                    text: last,
                    success: true,
                    mutated: false,
                });
            }
            for call in calls {
                let output = self
                    .tools
                    .execute(&call)
                    .await
                    .unwrap_or_else(|error| error.to_string());
                messages.push(Message::tool(call.id, output));
            }
        }
        Ok(crate::tools::ToolOutput {
            text: format!("agent stopped after {max_rounds} rounds\n{last}"),
            success: false,
            mutated: false,
        })
    }
}
