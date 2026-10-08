use crate::error::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub index: usize,
    pub phase: Option<String>,
    pub text: String,
    pub checks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    File(String),
    Stdout { command: String, expected: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contract {
    pub items: Vec<Item>,
}

pub(crate) fn contract(markdown: &str) -> Result<Contract> {
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

impl Contract {
    #[must_use]
    pub fn missing(&self, workspace: &std::path::Path) -> Vec<String> {
        let mut gone = Vec::new();
        for item in &self.items {
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
}

pub fn parse(markdown: &str) -> Result<Vec<Step>> {
    let mut steps: Vec<Step> = Vec::new();
    let mut in_section = false;
    let mut phase: Option<String> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_end();
        if let Some(heading) = trimmed.strip_prefix("### ") {
            if in_section {
                phase = Some(heading.trim().to_string());
            }
            continue;
        }
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
                phase: phase.clone(),
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

fn numbered(line: &str) -> Option<(usize, &str)> {
    let digits: String = line.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let rest = line.get(digits.len()..)?.strip_prefix(". ")?;
    digits.parse().ok().map(|number| (number, rest))
}

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

#[must_use]
pub fn prompt(step: &Step, total: usize, plan_path: &str) -> String {
    let mut text = format!(
        "Step {} of {} from {plan_path}. Do this step only; the earlier steps are done and the later ones are not yours yet. Implement exactly what it names. Add no extra flags, defaults, or features. Use the names it gives.\n\n{}\n",
        step.index, total, step.text
    );
    if !step.checks.is_empty() {
        let list: Vec<String> = step
            .checks
            .iter()
            .map(|check| format!("`{check}`"))
            .collect();
        text.push_str(&format!(
            "\nWhen you are done, run {} and make it pass. Summonr runs it again afterwards; a red check means the step is not finished.\n",
            list.join(" then ")
        ));
    }
    text
}

#[must_use]
pub fn exit_code(output: &str) -> Option<i32> {
    crate::tools::command_exit_code(output)
}
