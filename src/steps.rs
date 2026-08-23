//! Step mode: the harness holds the plan. It reads the numbered items under
//! a `## Steps` heading, hands the model one step at a time, and runs the
//! step's check itself before moving on.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const JOB_VERSION: u8 = 1;

/// One numbered step from the plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// 1-based position in the list.
    pub index: usize,
    /// The step text with its number removed.
    pub text: String,
    /// Every command named as `Run \`…\`` in the step, in order. All must pass.
    pub checks: Vec<String>,
}

/// The terminal outcome of a durable step job.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum Terminal {
    Completed,
    Failed { code: i32 },
    Aborted { reason: String },
}

impl Terminal {
    /// Process exit code represented by this terminal state.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Completed => 0,
            Self::Failed { code } => *code,
            Self::Aborted { .. } => 3,
        }
    }
}

/// Durable state for one `--steps` execution.
///
/// The state is deliberately tied to one canonical workspace, plan, and
/// session. A changed plan, workspace, or verified Git commit is not a
/// resumable job; callers must stop instead of guessing how to continue it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct JobState {
    pub version: u8,
    pub workspace: String,
    pub plan: String,
    pub plan_hash: String,
    /// The objective text when this is a goal job (`--goal`), else `None`.
    #[serde(default)]
    pub goal: Option<String>,
    /// Prompt to use for the next goal turn.
    #[serde(default)]
    pub goal_pending: Option<String>,
    /// Number of consecutive goal turns with a failed check.
    #[serde(default)]
    pub goal_red_streak: u32,
    /// Continuation limit for the goal job.
    #[serde(default)]
    pub goal_max_turns: Option<usize>,
    pub session: String,
    /// One-based ordinal of the next plan item — or completed goal turn + 1.
    pub next_step: usize,
    /// One-based ordinal of the last plan item that completed.
    pub last_completed_step: Option<usize>,
    pub spent_tokens: u64,
    pub elapsed_secs: u64,
    /// Git HEAD when available, or a canonical-path identity otherwise.
    pub workspace_identity: String,
    pub max_tokens: Option<u64>,
    pub max_secs: Option<u64>,
    pub verify: Option<String>,
    pub terminal: Option<Terminal>,
}

/// Handle for updating one durable step job.
pub struct Job {
    path: PathBuf,
    workspace: PathBuf,
    state: JobState,
}

impl Job {
    /// Load an existing state or create the state for a new session.
    pub fn open(
        workspace: &Path,
        plan_path: &Path,
        session: &str,
        max_tokens: Option<u64>,
        max_secs: Option<u64>,
        verify: Option<&str>,
    ) -> Result<Self> {
        let workspace = canonical(workspace, "workspace")?;
        let plan = canonical(plan_path, "plan")?;
        let plan_hash = std::fs::read_to_string(&plan)
            .map(|text| digest(&text))
            .map_err(|error| Error::Config(format!("cannot read {}: {error}", plan.display())))?;
        Self::resume(
            workspace,
            StateKey::Plan {
                path: plan.display().to_string(),
                hash: plan_hash,
            },
            None,
            session,
            max_tokens,
            max_secs,
            verify,
        )
    }

    /// Load an existing goal state or create one for a new session. The
    /// verify key binds the resume to the gate list that started the job.
    pub fn open_goal(
        workspace: &Path,
        goal: &str,
        session: &str,
        max_tokens: Option<u64>,
        max_secs: Option<u64>,
        verify_key: Option<&str>,
        max_turns: usize,
    ) -> Result<Self> {
        let workspace = canonical(workspace, "workspace")?;
        Self::resume(
            workspace,
            StateKey::Goal {
                text: goal.to_string(),
            },
            Some(max_turns),
            session,
            max_tokens,
            max_secs,
            verify_key,
        )
    }

    fn resume(
        workspace: PathBuf,
        key: StateKey,
        goal_max_turns: Option<usize>,
        session: &str,
        max_tokens: Option<u64>,
        max_secs: Option<u64>,
        verify: Option<&str>,
    ) -> Result<Self> {
        let identity = match &key {
            StateKey::Plan { path, hash } => (path.clone(), hash.clone()),
            StateKey::Goal { text } => (String::new(), digest(text)),
        };
        let goal = match &key {
            StateKey::Goal { text } => Some(text.clone()),
            StateKey::Plan { .. } => None,
        };
        let state = JobState {
            version: JOB_VERSION,
            workspace: workspace.display().to_string(),
            plan: identity.0,
            plan_hash: identity.1,
            goal,
            goal_pending: None,
            goal_red_streak: 0,
            goal_max_turns,
            session: session.to_string(),
            next_step: 1,
            last_completed_step: None,
            spent_tokens: 0,
            elapsed_secs: 0,
            workspace_identity: workspace_identity(&workspace),
            max_tokens,
            max_secs,
            verify: verify.map(str::to_string),
            terminal: None,
        };
        Ok(Self {
            path: PathBuf::new(),
            workspace,
            state,
        })
    }

    /// Progress lives on the session journal. There is no `.mage/jobs` file.
    pub fn load(_workspace: &Path, _plan_path: &Path) -> Result<Option<JobState>> {
        Ok(None)
    }

    /// Progress lives on the session journal. There is no `.mage/jobs` file.
    pub fn load_goal(_workspace: &Path, _goal: &str) -> Result<Option<JobState>> {
        Ok(None)
    }

    /// Restore a job from a journaled snapshot.
    #[must_use]
    pub fn from_state(workspace: PathBuf, state: JobState) -> Self {
        Self {
            path: PathBuf::new(),
            workspace,
            state,
        }
    }

    /// State as it will be written on the next update.
    #[must_use]
    pub fn state(&self) -> &JobState {
        &self.state
    }

    /// Path of the state file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reject a state that cannot correspond to this parsed plan.
    pub fn validate_steps(&self, total: usize) -> Result<()> {
        if self.state.next_step == 0 || self.state.next_step > total.saturating_add(1) {
            return Err(Error::Config(format!(
                "step job next step {} is outside the plan ({} steps)",
                self.state.next_step, total
            )));
        }
        if self
            .state
            .last_completed_step
            .is_some_and(|step| step >= self.state.next_step)
        {
            return Err(Error::Config(
                "step job completion is ahead of its next step".to_string(),
            ));
        }
        Ok(())
    }

    /// Mark one step complete and persist cumulative accounting.
    pub fn complete_step(
        &mut self,
        ordinal: usize,
        spent_tokens: u64,
        elapsed_secs: u64,
    ) -> Result<()> {
        if ordinal != self.state.next_step {
            return Err(Error::Config(format!(
                "step job expected step {}, got {ordinal}",
                self.state.next_step
            )));
        }
        let mut state = self.state.clone();
        state.last_completed_step = Some(ordinal);
        state.next_step = ordinal.saturating_add(1);
        update_accounting(&mut state, spent_tokens, elapsed_secs);
        state.workspace_identity = workspace_identity(&self.workspace);
        self.commit(state)
    }

    /// Persist accounting without advancing the plan.
    pub fn progress(&mut self, spent_tokens: u64, elapsed_secs: u64) -> Result<()> {
        let mut state = self.state.clone();
        update_accounting(&mut state, spent_tokens, elapsed_secs);
        state.workspace_identity = workspace_identity(&self.workspace);
        self.commit(state)
    }

    /// Complete one goal turn and save its next action in one state write.
    pub fn complete_goal(
        &mut self,
        ordinal: usize,
        pending: Option<String>,
        red_streak: u32,
        terminal: Option<Terminal>,
        spent_tokens: u64,
        elapsed_secs: u64,
    ) -> Result<()> {
        if self.state.goal.is_none() {
            return Err(Error::Config(
                "cannot save goal progress in a step job".to_string(),
            ));
        }
        if ordinal != self.state.next_step {
            return Err(Error::Config(format!(
                "goal job expected turn {}, got {ordinal}",
                self.state.next_step
            )));
        }
        let mut state = self.state.clone();
        state.last_completed_step = Some(ordinal);
        state.next_step = ordinal.saturating_add(1);
        state.goal_pending = pending;
        state.goal_red_streak = red_streak;
        state.terminal = terminal;
        update_accounting(&mut state, spent_tokens, elapsed_secs);
        state.workspace_identity = workspace_identity(&self.workspace);
        self.commit(state)
    }

    /// Mark the job terminal and persist cumulative accounting.
    pub fn finish(
        &mut self,
        terminal: Terminal,
        spent_tokens: u64,
        elapsed_secs: u64,
    ) -> Result<()> {
        let mut state = self.state.clone();
        update_accounting(&mut state, spent_tokens, elapsed_secs);
        state.goal_pending = None;
        state.terminal = Some(terminal);
        state.workspace_identity = workspace_identity(&self.workspace);
        self.commit(state)
    }

    fn commit(&mut self, state: JobState) -> Result<()> {
        self.state = state;
        Ok(())
    }
}

fn update_accounting(state: &mut JobState, spent_tokens: u64, elapsed_secs: u64) {
    state.spent_tokens = spent_tokens;
    state.elapsed_secs = elapsed_secs;
}

fn canonical(path: &Path, kind: &str) -> Result<PathBuf> {
    path.canonicalize().map_err(|error| {
        Error::Config(format!("cannot resolve {kind} {}: {error}", path.display()))
    })
}

/// What a durable job is keyed on: a plan file or a literal objective.
#[derive(Clone, Debug, PartialEq, Eq)]
enum StateKey {
    Plan { path: String, hash: String },
    Goal { text: String },
}

fn workspace_identity(workspace: &Path) -> String {
    let head = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|head| !head.is_empty());
    head.map_or_else(
        || format!("path:{}", workspace.display()),
        |head| format!("git:{head}"),
    )
}

fn digest(text: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// One literal requirement from the plan's `## Contract` section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// A file that must exist (workspace-relative).
    File(String),
    /// A command whose stdout must contain the literal.
    Stdout { command: String, expected: String },
}

/// A plan's contract: artifacts and executable observations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contract {
    pub items: Vec<Item>,
}

/// Parse the `## Contract` section. Bullet forms:
/// `- file \`path\`` or `- stdout \`command\` → \`expected\``.
/// Source-text checks are rejected because they do not prove behavior.
pub fn contract(markdown: &str) -> Result<Contract> {
    let mut items = Vec::new();
    let mut in_section = false;
    for line in markdown.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if in_section {
                break;
            }
            in_section = heading.trim().eq_ignore_ascii_case("contract");
            continue;
        }
        if !in_section {
            continue;
        }
        let Some(rest) = line.trim_start().strip_prefix("- ") else {
            continue;
        };
        let (kind, tail) = match rest.split_once(' ') {
            Some(parts) => parts,
            None => continue,
        };
        let quoted: Vec<&str> = tail.split('`').skip(1).step_by(2).collect();
        match (kind.trim(), quoted.as_slice()) {
            ("file", [path, ..]) => items.push(Item::File((*path).to_string())),
            ("stdout", [command, expected, ..]) => items.push(Item::Stdout {
                command: (*command).to_string(),
                expected: (*expected).to_string(),
            }),
            ("test" | "text", _) => {
                return Err(Error::Config(
                    "contract supports only `file` and `stdout`; use a command to prove behavior"
                        .to_string(),
                ));
            }
            _ => {}
        }
    }
    Ok(Contract { items })
}

/// Items the workspace does not satisfy. `Stdout` items need a shell and
/// are judged by the caller through [`stdout_item`].
#[must_use]
pub fn missing(contract: &Contract, workspace: &std::path::Path) -> Vec<String> {
    let mut gone = Vec::new();
    for item in &contract.items {
        match item {
            Item::File(path) => {
                if !workspace.join(path).exists() {
                    gone.push(format!("file `{path}` does not exist"));
                }
            }
            Item::Stdout { .. } => {}
        }
    }
    gone
}

/// Judge one `stdout` item from the command's captured result.
#[must_use]
pub fn stdout_item(command: &str, expected: &str, output: &str) -> Option<String> {
    if output.contains(expected) {
        None
    } else {
        Some(format!("`{command}` did not print `{expected}`"))
    }
}

/// The follow-up prompt when the contract is not met.
#[must_use]
pub fn contract_prompt(gone: &[String]) -> String {
    let list: Vec<String> = gone.iter().map(|item| format!("- {item}")).collect();
    format!(
        "The plan's Contract is not met. Fix these exactly as named — rename, do not add alternatives:\n{}\n",
        list.join("\n")
    )
}

/// Parse the numbered list under the `## Steps` heading of a Markdown
/// plan. Continuation lines belong to the item above them; the list ends
/// at the next heading.
pub fn parse(markdown: &str) -> Result<Vec<Step>> {
    let mut steps: Vec<Step> = Vec::new();
    let mut in_section = false;
    for line in markdown.lines() {
        let trimmed = line.trim_end();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            if in_section {
                break;
            }
            in_section = heading.trim().eq_ignore_ascii_case("steps");
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((number, rest)) = numbered(trimmed) {
            steps.push(Step {
                index: number,
                text: rest.trim().to_string(),
                checks: Vec::new(),
            });
        } else if let Some(last) = steps.last_mut()
            && !trimmed.trim().is_empty()
        {
            last.text.push(' ');
            last.text.push_str(trimmed.trim());
        }
    }
    if steps.is_empty() {
        return Err(Error::Config(
            "the plan has no numbered items under a `## Steps` heading".to_string(),
        ));
    }
    for step in &mut steps {
        step.checks = checks_in(&step.text);
    }
    Ok(steps)
}

/// `N. text` → `(N, text)`.
fn numbered(line: &str) -> Option<(usize, &str)> {
    let digits: String = line.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let rest = line.get(digits.len()..)?.strip_prefix(". ")?;
    digits.parse().ok().map(|number| (number, rest))
}

/// Every command in a `Run \`…\`` phrase of a step, in order.
fn checks_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(position) = rest.find("Run `") {
        let after = &rest[position + 5..];
        let Some(end) = after.find('`') else {
            break;
        };
        let command = after[..end].trim();
        if !command.is_empty() && !found.iter().any(|have| have == command) {
            found.push(command.to_string());
        }
        rest = &after[end + 1..];
    }
    found
}

/// The prompt that hands one step to the model.
#[must_use]
pub fn prompt(step: &Step, total: usize, plan_path: &str) -> String {
    let mut text = format!(
        "Step {} of {} from {plan_path}. Do this step only; the earlier steps are done and the later ones are not yours yet. Implement exactly what it names — no extra flags, defaults, or features — and use the names it gives.\n\n{}\n",
        step.index, total, step.text
    );
    if !step.checks.is_empty() {
        let list: Vec<String> = step
            .checks
            .iter()
            .map(|check| format!("`{check}`"))
            .collect();
        text.push_str(&format!(
            "\nWhen you are done, run {} and make it pass. Summoner runs it again afterwards; a red check means the step is not finished.\n",
            list.join(" then ")
        ));
    }
    text
}

/// The follow-up prompt after a failed check.
#[must_use]
pub fn retry_prompt(check: &str, output: &str) -> String {
    let tail: String = output
        .lines()
        .rev()
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    format!("The check `{check}` failed. Fix the cause, then run it again.\n\n{tail}\n")
}

/// Exit code from a `run_command` result: the first `exit N` line, which
/// may sit below a harness note.
#[must_use]
pub fn exit_code(output: &str) -> Option<i32> {
    crate::tools::command_exit_code(output)
}

#[cfg(test)]
#[path = "steps_tests.rs"]
mod tests;
