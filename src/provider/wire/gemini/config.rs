use serde_json::{Map, Value, json};

use crate::provider::wire::traits::{Thinking, traits};
use crate::provider::{CompletionRequest, Effort, Step};

pub(super) fn generation(call: &CompletionRequest<'_>) -> Value {
    let mut config = Map::new();
    if let Some(limit) = call.max_output_tokens {
        config.insert("maxOutputTokens".to_string(), json!(limit));
    }
    if let Some(thinking) = thinking(call) {
        config.insert("thinkingConfig".to_string(), thinking);
    }
    Value::Object(config)
}

fn thinking(call: &CompletionRequest<'_>) -> Option<Value> {
    let thoughts = call.effort != Some(Effort::None);
    let step = call.reasoning.plan(&call.model).step(call.effort);
    match (traits(&call.model).thinking, step) {
        (Thinking::Budget, Step::Level(level)) => {
            Some(json!({"thinkingBudget": budget(level), "includeThoughts": thoughts}))
        }
        (Thinking::Budget, Step::Off(_)) => Some(json!({"thinkingBudget": 0})),
        (Thinking::Level, Step::Level(level)) => {
            Some(json!({"thinkingLevel": level.as_str(), "includeThoughts": thoughts}))
        }
        (Thinking::Budget | Thinking::Level, Step::Default) | (Thinking::Level, Step::Off(_)) => {
            None
        }
        (
            Thinking::Adaptive | Thinking::Budgeted | Thinking::EffortBudget | Thinking::None,
            Step::Default | Step::Level(_) | Step::Off(_),
        ) => None,
    }
}

fn budget(level: Effort) -> i32 {
    match level {
        Effort::None => 0,
        Effort::Minimal | Effort::Low => 1024,
        Effort::Medium => 8192,
        Effort::High | Effort::XHigh | Effort::Max => -1,
    }
}
