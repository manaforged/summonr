use std::path::Path;

use serde::Deserialize;

use super::Metric;
use summonr_core::error::{Error, Result};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckFile {
    check: Option<String>,
    checks: Option<Vec<String>>,
    metric: Option<Metric>,
}

fn check_file(workspace: &Path) -> Result<Option<CheckFile>> {
    let path = check_path(workspace);
    let text = crate::durable::store::read_or_empty(&path)?;
    if text.is_empty() {
        return Ok(None);
    }
    let file: CheckFile = toml::from_str(&text)
        .map_err(|error| Error::Config(format!("{}: {error}", path.display())))?;
    if file.check.is_some() && file.checks.is_some() {
        return Err(Error::Config(format!(
            "{}: use `check` or `checks`, not both",
            path.display()
        )));
    }
    if let Some(pattern) = file
        .metric
        .as_ref()
        .and_then(|metric| metric.pattern.as_ref())
    {
        regex::Regex::new(pattern).map_err(|error| {
            Error::Config(format!("{}: metric pattern: {error}", path.display()))
        })?;
    }
    Ok(Some(file))
}

pub(crate) fn metric(workspace: &Path) -> Result<Option<Metric>> {
    Ok(check_file(workspace)?.and_then(|file| file.metric))
}

#[must_use]
pub(crate) fn check_path(workspace: &Path) -> std::path::PathBuf {
    workspace.join(".summonr/check.toml")
}

pub fn check_commands(workspace: &Path) -> Result<Option<Vec<String>>> {
    let Some(file) = check_file(workspace)? else {
        return Ok(None);
    };
    let metric = file.metric.is_some();
    let mut commands = match (file.check, file.checks) {
        (Some(check), _) => vec![check],
        (None, Some(checks)) => checks,
        (None, None) => Vec::new(),
    };
    commands.retain(|command| !command.trim().is_empty());
    Ok((metric || !commands.is_empty()).then_some(commands))
}

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;
