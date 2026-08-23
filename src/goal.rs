//! Goal mode: the harness holds the objective. One standing goal, run to
//! completion across turns — the model works, deterministic gates prove
//! progress, and a judge round decides whether the goal is met. The gate
//! verdict is mechanical (exit 0 or not); the judge only decides whether
//! to continue, never whether the work passed.
//!
//! State is durable under `.mage/jobs/` through the same job machinery as
//! step mode, so `summon --goal …` resumes after a process exit.

use crate::Turn;
use crate::error::{Error, Result};
use crate::provider::Provider;
use crate::runtime::Loop;
use crate::steps::{self, Terminal};

/// Continuation turns allowed before the goal auto-pauses.
pub const DEFAULT_MAX_TURNS: usize = 20;
/// Consecutive red-gate turns tolerated before the run gives up.
const GATE_STRIKES: u32 = 3;
/// How much of the agent's final message the judge sees.
const JUDGE_CONTEXT_CHARS: usize = 4_000;

const JUDGE_SYSTEM: &str = "You judge whether a coding objective is complete. Reply with one \
line of JSON: {\"done\": true|false, \"reason\": \"<one sentence>\"}. Mark done only when the \
response shows concrete evidence — a command result, file, or test output — that the objective \
is met. When it is not met, name what remains.";

/// Decision state and durability for one goal run. All decisions live
/// here; callers only execute turns and render notes.
pub struct Engine {
    job: steps::Job,
    objective: String,
    gates: Vec<String>,
    max_turns: usize,
    /// Prompt queued for the next turn, if the goal continues.
    pending: Option<String>,
    red_streak: u32,
    notes: Vec<String>,
}

impl Engine {
    /// Restore a goal engine from a journaled job snapshot.
    pub fn from_state(
        workspace: &std::path::Path,
        objective: &str,
        gates: Vec<String>,
        max_turns: usize,
        state: steps::JobState,
    ) -> Result<Self> {
        if objective.trim().is_empty() {
            return Err(Error::Config("the goal is empty".to_string()));
        }
        if state
            .goal_max_turns
            .is_some_and(|stored| stored != max_turns)
        {
            return Err(Error::Config(
                "job configuration changed; resume with the original limits, turn limit, and verify gate"
                    .to_string(),
            ));
        }
        let pending = state.goal_pending.clone();
        let red_streak = state.goal_red_streak;
        Ok(Self {
            job: steps::Job::from_state(workspace.to_path_buf(), state),
            objective: objective.to_string(),
            gates,
            max_turns,
            pending,
            red_streak,
            notes: Vec::new(),
        })
    }

    /// Open (or resume) the durable goal job for this session.
    pub fn open(
        workspace: &std::path::Path,
        objective: &str,
        session: &str,
        gates: Vec<String>,
        max_turns: usize,
        budget: &crate::Budget,
    ) -> Result<Self> {
        if objective.trim().is_empty() {
            return Err(Error::Config("the goal is empty".to_string()));
        }
        // The gate list binds the resume: changing check.toml invalidates
        // an in-flight goal just like changing a plan does.
        let verify_key = gates.join("\n");
        let verify_key = (!verify_key.is_empty()).then_some(verify_key.as_str());
        let job = steps::Job::open_goal(
            workspace,
            objective,
            session,
            budget.max_tokens,
            budget.max_secs,
            verify_key,
            max_turns,
        )?;
        let pending = job.state().goal_pending.clone();
        let red_streak = job.state().goal_red_streak;
        Ok(Self {
            job,
            objective: objective.to_string(),
            gates,
            max_turns,
            pending,
            red_streak,
            notes: Vec::new(),
        })
    }

    /// Load the durable state for a goal before a session is selected.
    pub fn load(workspace: &std::path::Path, objective: &str) -> Result<Option<steps::JobState>> {
        steps::Job::load_goal(workspace, objective)
    }

    /// Journaled job snapshot.
    #[must_use]
    pub fn snapshot(&self) -> &steps::JobState {
        self.job.state()
    }

    /// Human-readable progress lines since the last drain.
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// The prompt for the next turn, if the goal still wants one.
    pub fn prompt(&mut self) -> Result<Option<String>> {
        if self.job.state().terminal.is_some() {
            return Ok(None);
        }
        let turn = self.job.state().next_step;
        if turn > self.max_turns {
            let reason = format!("goal used its {turn} continuation turns");
            self.finish(Terminal::Aborted { reason })?;
            return Ok(None);
        }
        Ok(Some(self.pending.clone().unwrap_or_else(|| {
            opening_prompt(&self.objective, &self.gates)
        })))
    }

    /// Process one finished turn: persist accounting, run the gates, ask
    /// the judge, and queue the next prompt or a terminal state.
    pub async fn after_turn<P: Provider>(
        &mut self,
        loop_: &mut Loop<P>,
        turn: &Turn,
    ) -> Result<()> {
        let index = self.job.state().next_step;
        if turn.aborted {
            self.finish(Terminal::Aborted {
                reason: "goal turn aborted".to_string(),
            })?;
            return Ok(());
        }

        // Gates are the mechanical half of "done": every turn must leave
        // them green. A red gate feeds its output back verbatim.
        let mut failed = None;
        for gate in &self.gates {
            let output = loop_
                .shell(gate)
                .await
                .unwrap_or_else(|error| format!("exit 1\n{error}"));
            if steps::exit_code(&output) == Some(0) {
                self.notes.push(format!("[goal] check passed · {gate}"));
                continue;
            }
            self.notes.push(format!("[goal] check failed · {gate}"));
            failed = Some((gate.clone(), output));
            break;
        }
        if let Some((gate, output)) = failed {
            self.red_streak += 1;
            if self.red_streak >= GATE_STRIKES {
                self.notes.push(format!(
                    "[goal] gave up after {GATE_STRIKES} red turns · {gate}"
                ));
                self.pending = None;
                self.complete(loop_, index, Some(Terminal::Failed { code: 2 }))?;
                return Ok(());
            }
            self.pending = Some(steps::retry_prompt(&gate, &output));
            self.complete(loop_, index, None)?;
            return Ok(());
        }
        self.red_streak = 0;

        // Judge: fail-open. A broken judge continues; the turn budget and
        // the gates are the real backstops.
        let verdict = match loop_
            .aux(JUDGE_SYSTEM, &judge_user(&self.objective, &turn.text))
            .await
        {
            Ok(reply) => parse_verdict(&reply),
            Err(error) => {
                self.notes
                    .push(format!("[goal] judge unavailable — continuing ({error})"));
                Verdict::continue_(format!("judge error: {error}"))
            }
        };
        if verdict.done {
            self.notes
                .push(format!("[goal] achieved — {}", verdict.reason));
            self.pending = None;
            self.complete(loop_, index, Some(Terminal::Completed))?;
        } else {
            self.notes
                .push(format!("[goal] continuing — {}", verdict.reason));
            self.pending = Some(continuation_prompt(&verdict.reason));
            self.complete(loop_, index, None)?;
        }
        Ok(())
    }

    /// The process exit code once a terminal state exists.
    #[must_use]
    pub fn exit_code(&self) -> Option<i32> {
        self.job.state().terminal.as_ref().map(Terminal::exit_code)
    }

    fn complete<P: Provider>(
        &mut self,
        loop_: &mut Loop<P>,
        index: usize,
        terminal: Option<Terminal>,
    ) -> Result<()> {
        self.job.complete_goal(
            index,
            self.pending.clone(),
            self.red_streak,
            terminal,
            loop_.spent_tokens(),
            loop_.elapsed_secs(),
        )?;
        loop_.persist_progress(self.job.state())
    }

    fn finish(&mut self, terminal: Terminal) -> Result<()> {
        self.pending = None;
        let spent = self.job.state().spent_tokens;
        let elapsed = self.job.state().elapsed_secs;
        self.job.finish(terminal, spent, elapsed)
    }
}

fn opening_prompt(objective: &str, gates: &[String]) -> String {
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

fn continuation_prompt(reason: &str) -> String {
    format!(
        "[summon goal: the judge reviewed your work and the objective is not met — {reason}. \
Continue with the next concrete action.]"
    )
}

fn judge_user(objective: &str, reply: &str) -> String {
    let tail: String = reply
        .chars()
        .rev()
        .take(JUDGE_CONTEXT_CHARS)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("Objective:\n{objective}\n\nAgent's latest response:\n{tail}")
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

/// Parse the judge's one-line JSON. Prose around the object is tolerated;
/// anything unparsable counts as "continue" with the raw text as reason.
fn parse_verdict(reply: &str) -> Verdict {
    let trimmed = reply.trim();
    let Some(start) = trimmed.find('{') else {
        return Verdict::continue_(trimmed.chars().take(200).collect());
    };
    let Some(end) = trimmed.rfind('}') else {
        return Verdict::continue_(trimmed.chars().take(200).collect());
    };
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
