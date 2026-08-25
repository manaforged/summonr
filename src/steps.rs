//! Step mode: the harness holds the plan. It reads the numbered items under
//! a `## Steps` heading, hands the model one step at a time, and runs the
//! step's check itself before moving on.

use crate::error::{Error, Result};

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
