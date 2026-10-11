use super::*;

pub(crate) fn roman(number: usize) -> String {
    const DIGITS: [(usize, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut rest = number;
    let mut text = String::new();
    for (value, glyph) in DIGITS {
        while rest >= value {
            text.push_str(glyph);
            rest -= value;
        }
    }
    text
}

pub(crate) fn checks(
    markdown: &str,
    policy: Policy,
    commands: &[String],
) -> Result<Vec<Check>, WorkError> {
    let mut checks: Vec<Check> = commands
        .iter()
        .map(|command| Check {
            command: command.clone(),
            observation: None,
        })
        .collect();
    if policy != Policy::Steps {
        return Ok(checks);
    }
    let mut push = |command: String, observation: Option<Observation>| {
        if !checks
            .iter()
            .any(|known| known.command == command && known.observation == observation)
        {
            checks.push(Check {
                command,
                observation,
            });
        }
    };
    for command in crate::steps::parse(markdown)?
        .iter()
        .flat_map(|step| &step.checks)
    {
        push(command.clone(), None);
    }
    for item in crate::steps::contract(markdown)?.items {
        match item {
            crate::steps::Item::File(path) => {
                push(
                    format!("file exists: {path}"),
                    Some(Observation::File(path)),
                );
            }
            crate::steps::Item::Stdout { command, expected } => {
                push(command, Some(Observation::Stdout(expected)));
            }
        }
    }
    Ok(checks)
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Single,
    Steps,
    Goal,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPhase {
    Plan,
    Execute,
    Verify,
    Review,
    Completed,
    Failed,
}

impl Display for WorkPhase {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Plan => "planning",
            Self::Execute => "running",
            Self::Verify => "verifying",
            Self::Review => "reviewing",
            Self::Completed => "completed",
            Self::Failed => "failed",
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct WorkProgress {
    pub kind: WorkKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub objective: String,
    pub phase: WorkPhase,
    #[serde(default)]
    pub checks_failed: bool,
    pub current: Option<usize>,
    pub total: usize,
    pub attempts: u16,
    pub rounds: u32,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ledger: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub todos: Vec<crate::tools::Todo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub touched: Vec<String>,
}

impl WorkProgress {
    #[must_use]
    pub fn terminal(&self) -> bool {
        matches!(self.phase, WorkPhase::Completed | WorkPhase::Failed)
    }

    #[must_use]
    pub fn exit_code(&self) -> Option<i32> {
        match self.phase {
            WorkPhase::Completed => Some(0),
            WorkPhase::Failed if self.checks_failed => Some(2),
            WorkPhase::Failed => Some(3),
            WorkPhase::Plan | WorkPhase::Execute | WorkPhase::Verify | WorkPhase::Review => None,
        }
    }
}

impl Display for WorkProgress {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match (self.kind, self.current) {
            (WorkKind::Steps, Some(current)) => {
                write!(
                    formatter,
                    "Work step {current}/{} · {}",
                    self.total, self.phase
                )?;
            }
            (WorkKind::Goal, _) => {
                write!(formatter, "Work goal turn {} · {}", self.rounds, self.phase)?;
            }
            _ => write!(formatter, "Work · {}", self.phase)?,
        }
        if let Some(reason) = &self.reason {
            write!(formatter, " · {reason}")?;
        }
        Ok(())
    }
}
