use serde_json::{Value, json};

use crate::UserInput;
use crate::rpc;
use crate::runtime::{GrantAnswer, Steering};

const PERMISSION: &str = "permission-";

struct Choice {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    answer: GrantAnswer,
}

const CHOICES: [Choice; 3] = [
    Choice {
        id: "allow-once",
        name: "Allow once",
        kind: "allow_once",
        answer: GrantAnswer::Allow,
    },
    Choice {
        id: "allow-always",
        name: "Always allow",
        kind: "allow_always",
        answer: GrantAnswer::Always,
    },
    Choice {
        id: "reject-once",
        name: "Reject",
        kind: "reject_once",
        answer: GrantAnswer::Deny,
    },
];

/// The `session/request_permission` request for dialog `dialog`, naming the tool call that asks.
pub(super) fn request(
    session: &str,
    detail: &str,
    call: Option<&str>,
    persist: bool,
    dialog: u64,
) -> Value {
    let options = CHOICES
        .iter()
        .filter(|choice| persist || choice.answer != GrantAnswer::Always)
        .map(|choice| json!({"optionId": choice.id, "name": choice.name, "kind": choice.kind}))
        .collect::<Vec<_>>();
    let params = json!({
        "sessionId": session,
        "toolCall": {"toolCallId": call.unwrap_or_default(), "title": detail},
        "options": options,
    });
    rpc::request(
        &json!(format!("{PERMISSION}{dialog}")),
        "session/request_permission",
        params,
    )
}

/// Passes an editor's answer to permission dialog `id` to the session.
pub(super) fn answer(steering: &Steering, id: &Value, result: Option<&Value>) {
    let Some(dialog) = id
        .as_str()
        .and_then(|id| id.strip_prefix(PERMISSION))
        .and_then(|dialog| dialog.parse::<u64>().ok())
    else {
        return;
    };
    let outcome = result.and_then(|result| result.get("outcome"));
    let picked = outcome
        .filter(|outcome| outcome.get("outcome").and_then(Value::as_str) == Some("selected"))
        .and_then(|outcome| outcome.get("optionId"))
        .and_then(Value::as_str);
    let decision = CHOICES
        .iter()
        .find(|choice| Some(choice.id) == picked)
        .map_or(GrantAnswer::Deny, |choice| choice.answer);
    steering.answer(dialog, UserInput::from(decision.token()));
}
