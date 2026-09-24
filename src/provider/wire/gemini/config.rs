use serde_json::{Map, Value, json};

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
    if model.contains("gemini-2.5") {
        let budget = match effort {
            Effort::None if model.contains("pro") => return None,
            Effort::None => return Some(json!({"thinkingBudget": 0})),
            Effort::Low => 1024,
            Effort::Medium => 8192,
            Effort::High => -1,
        };
        return Some(json!({"thinkingBudget": budget, "includeThoughts": true}));
    }
    if !model.contains("gemini-3") {
        return None;
    }
    let minimal = !model.contains("pro") && !model.contains("3.7") && !model.contains("3.8");
    let level = match effort {
        Effort::None if minimal => "minimal",
        Effort::None | Effort::Low => "low",
        Effort::Medium if model.contains("gemini-3-pro") => "high",
        Effort::Medium => "medium",
        Effort::High => "high",
    };
    Some(json!({"thinkingLevel": level, "includeThoughts": effort != Effort::None}))
}
