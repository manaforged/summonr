use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use crate::config::{self, File, ModelSelectionRow};
use summonr_core::error::{Error, Result};

pub(crate) const RESERVED: [&str; 7] = ["off", "list", "show", "set", "clear", "save", "default"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Scout,
    Implement,
    Judge,
    Summary,
    Title,
}

impl Role {
    pub const ALL: [Self; 5] = [
        Self::Scout,
        Self::Implement,
        Self::Judge,
        Self::Summary,
        Self::Title,
    ];

    #[must_use]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Scout => "scout",
            Self::Implement => "implement",
            Self::Judge => "judge",
            Self::Summary => "summary",
            Self::Title => "title",
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelSelection {
    pub endpoint: String,
    pub model: Option<String>,
}

impl fmt::Display for ModelSelection {
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
    pub(crate) selections: BTreeMap<Role, ModelSelection>,
}

impl Workflow {
    /// A workflow named `name` that runs every role on the primary model.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.trim().to_string(),
            selections: BTreeMap::new(),
        }
    }

    /// The endpoint and model `role` runs on, or None for the primary model.
    #[must_use]
    pub fn selection(&self, role: Role) -> Option<&ModelSelection> {
        self.selections.get(&role)
    }

    /// Runs `role` on `selection`, or on the primary model when it is None.
    pub fn assign(&mut self, role: Role, selection: Option<ModelSelection>) {
        match selection {
            Some(selection) => drop(self.selections.insert(role, selection)),
            None => drop(self.selections.remove(&role)),
        }
    }

    #[must_use]
    pub fn summary(&self) -> String {
        if self.selections.is_empty() {
            return "every role on the primary model".to_string();
        }
        self.selections
            .iter()
            .map(|(role, selection)| format!("{role} {selection}"))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

pub fn names() -> Result<Vec<String>> {
    Ok(config::file()?.workflows.keys().cloned().collect())
}

pub fn default_name() -> Result<Option<String>> {
    Ok(config::file()?.workflow.clone())
}

pub fn load(name: &str) -> Result<Workflow> {
    from_file(&*config::file()?, name)
}

pub(crate) fn from_file(file: &File, name: &str) -> Result<Workflow> {
    let rows = file
        .workflows
        .get(name)
        .ok_or_else(|| Error::Config(format!("no workflow `{name}` in config.toml")))?;
    let mut selections = BTreeMap::new();
    for (role, row) in rows {
        let selection = match row {
            ModelSelectionRow::Name(endpoint) => ModelSelection {
                endpoint: endpoint.clone(),
                model: None,
            },
            ModelSelectionRow::Full { endpoint, model } => ModelSelection {
                endpoint: endpoint.clone(),
                model: model.clone(),
            },
        };
        if selection.endpoint.trim().is_empty() {
            return Err(Error::Config(format!(
                "workflow `{name}`: `{role}` names no endpoint"
            )));
        }
        selections.insert(role.parse()?, selection);
    }
    Ok(Workflow {
        name: name.to_string(),
        selections,
    })
}

/// Writes `workflow` to config.toml as `[workflows.NAME]`.
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

/// Deletes the saved workflow `name`, and clears the default when it was the default; false when there was none.
pub fn remove(name: &str) -> Result<bool> {
    let path = config::path()?;
    let removed = config::remove_named(&path, config::Section::Workflow, name)?;
    if removed && default_name()?.as_deref() == Some(name) {
        config::clear_default_workflow(&path)?;
    }
    Ok(removed)
}

/// Makes the saved workflow `name` the one new sessions start with.
pub fn set_default(name: &str) -> Result<()> {
    let path = config::path()?;
    from_file(&config::parse(&path)?, name)?;
    config::save_default_workflow(&path, name)
}
