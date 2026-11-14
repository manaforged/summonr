use serde_json::{Value, json};

use super::{Reply, codes};
use crate::provider::Effort;
use crate::runtime::Turn;
use crate::tools::{AbortCode, Event};

pub(super) const EFFORT: &str = "effort";
const DEFAULT: &str = "default";
pub(super) const CANCELLED: &str = "cancelled";
pub(super) const STOPS: [&str; 4] = ["end_turn", "max_tokens", "max_turn_requests", CANCELLED];

pub(super) fn values() -> Vec<&'static str> {
    std::iter::once(DEFAULT)
        .chain(Effort::ALL.iter().map(|level| level.as_str()))
        .collect()
}

pub(super) fn options(effort: Option<Effort>) -> Value {
    let mut values = vec![json!({"value": DEFAULT, "name": "Default"})];
    values.extend(
        Effort::ALL
            .iter()
            .map(|level| json!({"value": level.as_str(), "name": level.label()})),
    );
    json!([{
        "id": EFFORT,
        "name": "Effort",
        "category": "thought_level",
        "type": "select",
        "currentValue": effort.map_or(DEFAULT, Effort::as_str),
        "options": values,
    }])
}

pub(super) fn effort(params: &Value) -> std::result::Result<Option<Effort>, (i64, String)> {
    let id = params
        .get("configId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if id != EFFORT {
        return Err((codes::INVALID, format!("unknown configId `{id}`")));
    }
    let value = params
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if value == DEFAULT {
        return Ok(None);
    }
    Effort::parse(value)
        .map(Some)
        .ok_or_else(|| (codes::INVALID, format!("effort {}", Effort::rejects(value))))
}

pub(super) fn stop(turn: &Turn) -> Reply {
    if !turn.aborted {
        return Ok(json!({"stopReason": STOPS[0]}));
    }
    let (code, reason) = turn
        .events
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::Aborted { code, reason } => Some((*code, reason.as_str())),
            _other => None,
        })
        .unwrap_or((AbortCode::Error, "the turn stopped"));
    let reason = match code {
        AbortCode::TokenBudget => STOPS[1],
        AbortCode::TimeBudget => STOPS[2],
        AbortCode::Interrupted | AbortCode::Cancelled => CANCELLED,
        AbortCode::Error => return Err((codes::INTERNAL, reason.to_string())),
    };
    Ok(json!({"stopReason": reason}))
}
