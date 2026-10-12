use crate::durable::work::Decision;
use crate::provider::Provider;
use crate::runtime::Loop;
use crate::{Error, ProviderFailureKind, Result};

const REPAIRS: usize = 2;

const JUDGE_SYSTEM: &str = "You judge whether a coding objective is complete. Reply with one \
line of JSON: {\"verdict\": \"done\"|\"continue\"|\"stalled\"|\"impossible\", \"reason\": \
\"<one sentence>\"}. `done` only when the response shows concrete evidence (a command result, \
file, or test output) that the objective is met. Never `done` when the pass depends on hidden \
state, call order, special cases for test inputs, or changed test expectations, or on a \
changed environment for the check (a stub or shim on PATH, an edited file outside the \
workspace, a faked clock or tool); reply `continue` and name the trick. `continue` when work remains; name what remains. `stalled` when the response reports no concrete action on the objective. \
`impossible` when the objective cannot be met in this workspace; name the blocker. When two \
tests expect different results for the same input, reply `impossible` and name both tests.";

pub(crate) async fn decision<P: Provider>(
    loop_: &mut Loop<P>,
    objective: &str,
    reply: &str,
    evidence: &str,
    usage: &mut crate::provider::Usage,
) -> Decision {
    let evidence = judge_user(objective, reply, evidence);
    let mut prompt = evidence.clone();
    let mut repairs = 0;
    let verdict = loop {
        let (reply, judge_usage) = match loop_.judge(JUDGE_SYSTEM, &prompt).await {
            Ok(reply) => reply,
            Err(error) => return Decision::GoalContinue(format!("judge unavailable: {error}")),
        };
        usage.add(&judge_usage);
        match parse_verdict(&reply) {
            Ok(verdict) => break verdict,
            Err(error) if repairs >= REPAIRS => {
                return Decision::GoalContinue(format!(
                    "goal judge stayed malformed after {REPAIRS} repairs: {error}"
                ));
            }
            Err(_) => {
                repairs += 1;
                prompt = format!("{evidence}\n\n{}", repair_user(&reply));
            }
        }
    };
    match verdict.state {
        State::Done => Decision::GoalComplete,
        State::Continue => Decision::GoalContinue(verdict.reason),
        State::Stalled => Decision::GoalStop(format!("stalled: {}", verdict.reason)),
        State::Impossible => Decision::GoalStop(format!("impossible: {}", verdict.reason)),
    }
}

#[must_use]
pub fn prompt(objective: &str, gates: &[String], round: u32) -> String {
    if round > 0 {
        return "[summon goal: continue the active Work]".to_string();
    }
    let mut text = format!(
        "Work toward this objective. Keep going until it is fully met. Do not stop halfway or \
ask questions you can resolve yourself:\n\n{objective}\n"
    );
    if !gates.is_empty() {
        let list = gates
            .iter()
            .map(|gate| format!("`{gate}`"))
            .collect::<Vec<_>>()
            .join(" then ");
        text.push_str(&format!(
            "\nAfter each turn the harness runs {list}. A red check means the objective is not met.\n"
        ));
    }
    text
}

fn judge_user(objective: &str, reply: &str, evidence: &str) -> String {
    format!(
        "Objective:\n{objective}\n\nHarness evidence:\n{evidence}\n\nAgent's latest response:\n{reply}"
    )
}

fn repair_user(reply: &str) -> String {
    format!(
        "The previous response was invalid. Return exactly one verdict JSON object and no other \
         text. Previous response:\n{reply}"
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Done,
    Continue,
    Stalled,
    Impossible,
}

struct Verdict {
    state: State,
    reason: String,
}

fn parse_verdict(reply: &str) -> Result<Verdict> {
    let trimmed = reply.trim();
    let start = trimmed.find('{').ok_or_else(malformed_judge)?;
    let end = trimmed.rfind('}').ok_or_else(malformed_judge)?;
    if end < start {
        return Err(malformed_judge());
    }
    let value: serde_json::Value =
        serde_json::from_str(&trimmed[start..=end]).map_err(|error| {
            Error::provider(
                ProviderFailureKind::Malformed,
                format!("goal judge returned invalid JSON: {error}"),
            )
        })?;
    let state = match value.get("verdict").and_then(|verdict| verdict.as_str()) {
        Some("done") => State::Done,
        Some("continue") => State::Continue,
        Some("stalled") => State::Stalled,
        Some("impossible") => State::Impossible,
        Some(value) => {
            return Err(Error::provider(
                ProviderFailureKind::Malformed,
                format!("goal judge returned unknown verdict `{value}`"),
            ));
        }
        None => match value.get("done").and_then(serde_json::Value::as_bool) {
            Some(true) => State::Done,
            Some(false) => State::Continue,
            None => return Err(malformed_judge()),
        },
    };
    let reason = value
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(malformed_judge)?
        .to_string();
    Ok(Verdict { state, reason })
}

fn malformed_judge() -> Error {
    Error::provider(
        ProviderFailureKind::Malformed,
        "goal judge returned no valid verdict object",
    )
}
