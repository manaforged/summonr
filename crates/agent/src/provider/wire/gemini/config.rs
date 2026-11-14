use serde_json::{Map, Value, json};

use crate::provider::wire::traits::Thinking;
use crate::provider::{CompletionRequest, Effort, Reasoning, Step};

pub(super) fn generation(call: &CompletionRequest<'_>, reasoning: &Reasoning) -> Value {
    let mut config = Map::new();
    if let Some(limit) = call.max_output_tokens {
        config.insert("maxOutputTokens".to_string(), json!(limit));
    }
    if let Some(thinking) = thinking(call, reasoning) {
        config.insert("thinkingConfig".to_string(), thinking);
    }
    Value::Object(config)
}

fn thinking(call: &CompletionRequest<'_>, reasoning: &Reasoning) -> Option<Value> {
    let thoughts = call.effort != Some(Effort::None);
    let step = reasoning.plan(call.traits).step(call.effort);
    match (call.traits.thinking, step) {
        (Thinking::Budget, Step::Level(level)) => {
            Some(json!({"thinkingBudget": budget(call, level), "includeThoughts": thoughts}))
        }
        (Thinking::Budget, Step::Off(_)) => Some(json!({"thinkingBudget": 0})),
        (Thinking::Level, Step::Level(level)) => {
            Some(json!({"thinkingLevel": level.as_str(), "includeThoughts": thoughts}))
        }
        (Thinking::Budget | Thinking::Level, Step::Default) => {
            Some(json!({"includeThoughts": true}))
        }
        (Thinking::Level, Step::Off(_)) => None,
        (
            Thinking::Adaptive | Thinking::Budgeted | Thinking::EffortBudget | Thinking::None,
            Step::Default | Step::Level(_) | Step::Off(_),
        ) => None,
    }
}

fn budget(call: &CompletionRequest<'_>, level: Effort) -> i32 {
    call.traits
        .thinking_budgets
        .iter()
        .find_map(|(effort, budget)| (*effort == level).then_some(*budget))
        .unwrap_or(-1)
}
