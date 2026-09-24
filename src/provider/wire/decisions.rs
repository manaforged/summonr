use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::classify::{Decision, Question};
use crate::error::{Error, ProviderFailureKind, Result};

pub(crate) fn body(model: &str, state: &Value, questions: &BTreeMap<String, Question>) -> Value {
    json!({
        "model": model,
        "state": state,
        "questions": questions,
    })
}

pub(crate) fn decision(text: &str) -> Result<Decision> {
    serde_json::from_str(text).map_err(|error| {
        Error::provider(
            ProviderFailureKind::Malformed,
            format!("{error}: {}", text.chars().take(200).collect::<String>()),
        )
    })
}
