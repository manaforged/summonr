use crate::durable::work::Decision;
use crate::provider::Provider;
use crate::runtime::Loop;
use crate::{Error, ProviderFailureKind, Result};

const JUDGE_SYSTEM: &str = "You judge whether a coding objective is complete. Reply with one \
line of JSON: {\"verdict\": \"done\"|\"continue\"|\"stalled\"|\"impossible\", \"reason\": \
\"<one sentence>\"}. `done` only when the response shows concrete evidence (a command result, \
file, or test output) that the objective is met. Never `done` when the pass depends on hidden \
state, call order, special cases for test inputs, or changed test expectations, or on a \
changed environment for the check (a stub or shim on PATH, an edited file outside the \
workspace, a faked clock or tool); reply `continue` and name the trick. `continue` when work remains; name what remains. For an open-ended objective, `done` needs the outcome the agent committed to and the evidence that shows it, on something a user would notice; formatting, renames, comments, docs, a test run alone, or the agent's belief that it is done are `continue`, so name the bigger gap. `stalled` when the response reports no concrete action on the objective; a question the agent could answer from the workspace is `continue`, not `stalled`. \
`impossible` when the objective cannot be met in this workspace; name the blocker. When two \
tests expect different results for the same input, reply `impossible` and name both tests. \
A user message in the harness evidence overrides the objective; when it tells the agent to stop \
or leave the work, reply `done`.";

const CONTRACT_SYSTEM: &str = "You judge whether check commands are a fair contract for a \
coding objective. Reply with one line of JSON: {\"verdict\": \"done\"|\"continue\", \"reason\": \
\"<one sentence>\"}. `done` when the commands run real code that fails while the objective is \
unmet and passes only once it is met: a test that reproduces the defect, a property test, a model \
checker or proof, a benchmark with a target, or a mutation run on the diff. `continue` when a \
command passes regardless of the change (`true`, `echo`, a grep for a string the agent writes, a \
test that does not exercise the objective), when the commands check an easier outcome than the \
objective asks, or when the test files the commands depend on are not listed as protected; name \
what the contract misses.";

pub(crate) fn with_corrections(evidence: &str, events: &[crate::Event]) -> String {
    let said = events
        .iter()
        .filter_map(|event| match event {
            crate::Event::Steered { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if said.is_empty() {
        return evidence.to_string();
    }
    format!(
        "{evidence}\n\nMessages the user sent during this turn, in order:\n{}",
        said.join("\n")
    )
}

pub(crate) async fn decision<P: Provider>(
    loop_: &mut Loop<P>,
    objective: &str,
    reply: &str,
    evidence: &str,
    usage: &mut crate::provider::Usage,
) -> Decision {
    judged(loop_, objective, reply, evidence, usage)
        .await
        .unwrap_or_else(Decision::GoalContinue)
}

pub(crate) async fn judged<P: Provider>(
    loop_: &mut Loop<P>,
    objective: &str,
    reply: &str,
    evidence: &str,
    usage: &mut crate::provider::Usage,
) -> std::result::Result<Decision, String> {
    let verdict = ask(loop_, JUDGE_SYSTEM, judge_user(objective, reply, evidence), usage).await?;
    Ok(match verdict.state {
        State::Done => Decision::GoalComplete,
        State::Continue => Decision::GoalContinue(verdict.reason),
        State::Stalled => Decision::GoalStop(format!("stalled: {}", verdict.reason)),
        State::Impossible => Decision::GoalStop(format!("impossible: {}", verdict.reason)),
    })
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
    if gates.is_empty() {
        text.push_str(
            "\nNo check is set. Before changing anything, write the contract to `.summonr/contract.toml` as `checks = [\"CMD\", ...]` and `protect = [\"PATH\", ...]`: commands that fail now and pass only when the objective is met, commands that guard what must not regress, and the test files and fixtures those commands depend on. Then end the turn before you change the code: summonr runs the commands and needs at least one to fail. Pick the outcome that matters most to a user of this code, not the easiest one. Prefer evidence a tool computes: a test that reproduces the defect, a property test for an invariant, a model checker or proof for unsafe or arithmetic code, and a mutation run on the diff to show the tests catch the change. A judge then reads the contract; once it accepts, summonr freezes the commands and the protected files, runs the commands after each turn, and the goal ends when every command passes. Formatting, renames, comments, and docs alone are not an outcome.\n",
        );
    } else {
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

pub(crate) async fn contract<P: Provider>(
    loop_: &mut Loop<P>,
    objective: &str,
    contract: &crate::verify::Contract,
    usage: &mut crate::provider::Usage,
) -> std::result::Result<(), String> {
    let list = |items: &[String]| {
        if items.is_empty() {
            return "none".to_string();
        }
        items
            .iter()
            .map(|item| format!("- `{item}`"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let user = format!(
        "Objective:\n{objective}\n\nProposed check commands (at least one fails now):\n{}\n\nProtected files:\n{}",
        list(&contract.checks),
        list(&contract.protect)
    );
    let verdict = ask(loop_, CONTRACT_SYSTEM, user, usage).await?;
    match verdict.state {
        State::Done => Ok(()),
        State::Continue | State::Stalled | State::Impossible => Err(verdict.reason),
    }
}

async fn ask<P: Provider>(
    loop_: &mut Loop<P>,
    system: &str,
    evidence: String,
    usage: &mut crate::provider::Usage,
) -> std::result::Result<Verdict, String> {
    let mut prompt = evidence.clone();
    let mut repairs = 0;
    loop {
        let (reply, judge_usage) = match loop_.judge(system, &prompt).await {
            Ok(reply) => reply,
            Err(error) => return Err(format!("judge unavailable: {error}")),
        };
        usage.add(&judge_usage);
        match parse_verdict(&reply) {
            Ok(verdict) => return Ok(verdict),
            Err(error) if repairs >= crate::limits::limits().judge_repairs => {
                return Err(format!(
                    "goal judge stayed malformed after {} repairs: {error}",
                    crate::limits::limits().judge_repairs
                ));
            }
            Err(_) => {
                repairs += 1;
                prompt = format!("{evidence}\n\n{}", repair_user(&reply));
            }
        }
    }
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

#[derive(serde::Deserialize)]
struct Reply {
    verdict: Option<String>,
    done: Option<bool>,
    reason: Option<String>,
}

pub(crate) fn reply_object<T: serde::de::DeserializeOwned>(
    reply: &str,
) -> Option<serde_json::Result<T>> {
    let trimmed = reply.trim();
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    (start <= end).then(|| serde_json::from_str(&trimmed[start..=end]))
}

fn parse_verdict(reply: &str) -> Result<Verdict> {
    let reply: Reply = match reply_object(reply).ok_or_else(malformed_judge)? {
        Ok(reply) => reply,
        Err(error) if error.is_data() => return Err(malformed_judge()),
        Err(error) => {
            return Err(Error::provider(
                ProviderFailureKind::Malformed,
                format!("goal judge returned invalid JSON: {error}"),
            ));
        }
    };
    let state = match reply.verdict.as_deref() {
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
        None => match reply.done {
            Some(true) => State::Done,
            Some(false) => State::Continue,
            None => return Err(malformed_judge()),
        },
    };
    let reason = reply.reason.ok_or_else(malformed_judge)?;
    Ok(Verdict { state, reason })
}

fn malformed_judge() -> Error {
    Error::provider(
        ProviderFailureKind::Malformed,
        "goal judge returned no valid verdict object",
    )
}
