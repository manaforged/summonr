use crate::durable::work::Decision;
use crate::provider::Provider;
use crate::runtime::Loop;

pub const DEFAULT_MAX_TURNS: usize = 20;
const JUDGE_CONTEXT_CHARS: usize = 4_000;

const JUDGE_SYSTEM: &str = "You judge whether a coding objective is complete. Reply with one \
line of JSON: {\"done\": true|false, \"reason\": \"<one sentence>\"}. Mark done only when the \
response shows concrete evidence — a command result, file, or test output — that the objective \
is met. When it is not met, name what remains.";

pub(crate) async fn decision<P: Provider>(
    loop_: &mut Loop<P>,
    objective: &str,
    reply: &str,
    evidence: &str,
    usage: &mut crate::provider::Usage,
) -> Decision {
    match loop_
        .judge(JUDGE_SYSTEM, &judge_user(objective, reply, evidence))
        .await
    {
        Ok((reply, judge_usage)) => {
            usage.add(&judge_usage);
            let verdict = parse_verdict(&reply);
            if verdict.done {
                Decision::GoalComplete
            } else {
                Decision::GoalContinue(verdict.reason)
            }
        }
        Err(error) => Decision::GoalContinue(format!("judge unavailable: {error}")),
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

struct Verdict {
    done: bool,
    reason: String,
}

impl Verdict {
    fn continue_(reason: String) -> Self {
        Self {
            done: false,
            reason,
        }
    }
}

fn parse_verdict(reply: &str) -> Verdict {
    let trimmed = reply.trim();
    let Some(start) = trimmed.find('{') else {
        return Verdict::continue_(trimmed.chars().take(200).collect());
    };
    let Some(end) = trimmed.rfind('}') else {
        return Verdict::continue_(trimmed.chars().take(200).collect());
    };
    if end < start {
        return Verdict::continue_(trimmed.chars().take(200).collect());
    }
    match serde_json::from_str::<serde_json::Value>(&trimmed[start..=end]) {
        Ok(value) => Verdict {
            done: matches!(value.get("done"), Some(serde_json::Value::Bool(true))),
            reason: value
                .get("reason")
                .and_then(|reason| reason.as_str())
                .unwrap_or("no reason given")
                .to_string(),
        },
        Err(_) => Verdict::continue_(trimmed.chars().take(200).collect()),
    }
}

#[cfg(test)]
#[path = "goal_tests.rs"]
mod tests;
