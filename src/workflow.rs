//! Workflows: one seat per step.
//!
//! A workflow names the endpoint, and optionally the model, that runs each
//! role. A role the workflow does not name uses the session's driving seat.
//! Workflows live in `~/.mage/config.toml` as `[workflows.NAME]` tables; the
//! top-level `workflow` key names the default.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use crate::config::{self, File, SeatRow};
use crate::error::{Error, Result};

/// Words `/workflow` reads as verbs; a workflow cannot take one as its name.
pub const RESERVED: [&str; 7] = ["off", "list", "show", "set", "clear", "save", "default"];

/// A step that calls a model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// Read-only reconnaissance before a job's first plan or turn.
    Scout,
    /// Plan-mode rounds.
    Plan,
    /// Execute rounds.
    Implement,
    /// Read-only review after the gates pass.
    Review,
    /// The goal judge.
    Judge,
    /// The compaction summary.
    Summary,
}

impl Role {
    pub const ALL: [Self; 6] = [
        Self::Scout,
        Self::Plan,
        Self::Implement,
        Self::Review,
        Self::Judge,
        Self::Summary,
    ];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Scout => "scout",
            Self::Plan => "plan",
            Self::Implement => "implement",
            Self::Review => "review",
            Self::Judge => "judge",
            Self::Summary => "summary",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl FromStr for Role {
    type Err = Error;

    fn from_str(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|role| role.name() == name)
            .ok_or_else(|| Error::Config(format!("unknown workflow role `{name}`")))
    }
}

/// An endpoint and, when set, a model on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seat {
    pub endpoint: String,
    pub model: Option<String>,
}

impl fmt::Display for Seat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.model {
            Some(model) => write!(formatter, "{} {model}", self.endpoint),
            None => formatter.write_str(&self.endpoint),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Workflow {
    pub name: String,
    pub seats: BTreeMap<Role, Seat>,
}

impl Workflow {
    #[must_use]
    pub fn seat(&self, role: Role) -> Option<&Seat> {
        self.seats.get(&role)
    }

    /// `role seat · role seat …`, for status lines.
    #[must_use]
    pub fn summary(&self) -> String {
        if self.seats.is_empty() {
            return "every role on the driving seat".to_string();
        }
        self.seats
            .iter()
            .map(|(role, seat)| format!("{role} {seat}"))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// Saved workflow names, in config order.
pub fn names() -> Result<Vec<String>> {
    Ok(config::load()?.workflows.keys().cloned().collect())
}

/// The workflow the config names as default, if any.
pub fn default_name() -> Result<Option<String>> {
    Ok(config::load()?.workflow)
}

pub fn load(name: &str) -> Result<Workflow> {
    from_file(&config::load()?, name)
}

pub(crate) fn from_file(file: &File, name: &str) -> Result<Workflow> {
    let rows = file
        .workflows
        .get(name)
        .ok_or_else(|| Error::Config(format!("no workflow `{name}` in config.toml")))?;
    let mut seats = BTreeMap::new();
    for (role, row) in rows {
        let seat = match row {
            SeatRow::Name(endpoint) => Seat {
                endpoint: endpoint.clone(),
                model: None,
            },
            SeatRow::Full { endpoint, model } => Seat {
                endpoint: endpoint.clone(),
                model: model.clone(),
            },
        };
        if seat.endpoint.trim().is_empty() {
            return Err(Error::Config(format!(
                "workflow `{name}`: `{role}` names no endpoint"
            )));
        }
        seats.insert(role.parse()?, seat);
    }
    Ok(Workflow {
        name: name.to_string(),
        seats,
    })
}

/// Write the workflow's `[workflows.NAME]` table, replacing any existing one.
pub fn save(workflow: &Workflow) -> Result<()> {
    let name = workflow.name.trim();
    if name.is_empty() {
        return Err(Error::Config("workflow name is empty".to_string()));
    }
    if RESERVED.contains(&name) {
        return Err(Error::Config(format!(
            "`{name}` is a /workflow verb, not a name"
        )));
    }
    config::save_workflow(&config::path()?, workflow)
}

/// Make a saved workflow the default for new sessions.
pub fn set_default(name: &str) -> Result<()> {
    let path = config::path()?;
    from_file(&config::load_from(&path)?, name)?;
    config::save_default_workflow(&path, name)
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;
