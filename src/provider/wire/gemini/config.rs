use serde_json::{Map, Value, json};

use crate::provider::wire::traits::{Thinking, traits};
use crate::provider::{CompletionRequest, Effort};

pub(super) fn generation(call: &CompletionRequest<'_>) -> Value {
    let mut config = Map::new();
    if let Some(limit) = call.max_output_tokens {
        config.insert("maxOutputTokens".to_string(), json!(limit));
    }
    if let Some(thinking) = call.effort.and_then(|effort| thinking(&call.model, effort)) {
        config.insert("thinkingConfig".to_string(), thinking);
    }
    Value::Object(config)
}

fn thinking(model: &str, effort: Effort) -> Option<Value> {
    match traits(model).thinking {
        Thinking::Budget { can_disable } => {
            let budget = match effort {
                Effort::None if !can_disable => return None,
                Effort::None => return Some(json!({"thinkingBudget": 0})),
                Effort::Low => 1024,
                Effort::Medium => 8192,
                Effort::High => -1,
            };
            Some(json!({"thinkingBudget": budget, "includeThoughts": true}))
        }
        Thinking::Level { minimal, medium } => {
            let level = match effort {
                Effort::None if minimal => "minimal",
                Effort::None | Effort::Low => "low",
                Effort::Medium if medium => "medium",
                Effort::Medium | Effort::High => "high",
            };
            Some(json!({"thinkingLevel": level, "includeThoughts": effort != Effort::None}))
        }
        Thinking::Adaptive | Thinking::Budgeted | Thinking::None => None,
    }
}
