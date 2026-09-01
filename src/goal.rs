use crate::durable::work::Decision;
use crate::provider::Provider;
use crate::runtime::Loop;
use crate::{Error, ProviderFailureKind, Result};

pub const DEFAULT_MAX_TURNS: usize = 20;
const JUDGE_CONTEXT_CHARS: usize = 4_000;

const JUDGE_SYSTEM: &str = "You judge whether a coding objective is complete. Reply with one \
line of JSON: {\"verdict\": \"done\"|\"continue\"|\"stalled\"|\"impossible\", \"reason\": \
\"<one sentence>\"}. `done` only when the response shows concrete evidence — a command result, \
file, or test output — that the objective is met. `continue` when work remains; name what \
remains. `stalled` when the response reports no concrete action on the objective. \
`impossible` when the objective cannot be met in this workspace; name the blocker.";

pub(crate) async fn decision<P: Provider>(
    loop_: &mut Loop<P>,
    objective: &str,
    reply: &str,
    evidence: &str,
    usage: &mut crate::provider::Usage,
) -> Decision {
    let (reply, judge_usage) = match loop_
        .judge(JUDGE_SYSTEM, &judge_user(objective, reply, evidence))
        .await
    {
        Ok(reply) => reply,
        Err(error) => return Decision::GoalContinue(format!("judge unavailable: {error}")),
    };
    usage.add(&judge_usage);
    let verdict = match parse_verdict(&reply) {
        Ok(verdict) => verdict,
        Err(first) => {
            let (reply, judge_usage) = match loop_.judge(JUDGE_SYSTEM, &repair_user(&reply)).await {
                Ok(reply) => reply,
                Err(error) => {
                    return Decision::GoalContinue(format!(
                        "judge repair unavailable after {first}: {error}"
                    ));
                }
            };
            usage.add(&judge_usage);
            match parse_verdict(&reply) {
                Ok(verdict) => verdict,
                Err(error) => {
                    return Decision::GoalContinue(format!(
                        "goal judge remained malformed after one repair: {error}"
                    ));
                }
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
        "Work toward this objective. Keep going until it is fully met — do not stop halfway or \
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
    let tail: String = reply
        .chars()
        .rev()
        .take(JUDGE_CONTEXT_CHARS)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!(
        "Objective:\n{objective}\n\nHarness evidence:\n{evidence}\n\nAgent's latest response:\n{tail}"
    )
}

fn repair_user(reply: &str) -> String {
    format!(
        "The previous response was invalid. Return exactly one verdict JSON object and no other \
         text. Previous response:\n{}",
        reply.chars().take(JUDGE_CONTEXT_CHARS).collect::<String>()
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

#[cfg(test)]
#[path = "goal_tests.rs"]
mod tests;
