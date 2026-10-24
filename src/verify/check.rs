use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

#[derive(Debug, Default, Deserialize)]
struct CheckFile {
    check: Option<String>,
    checks: Option<Vec<String>>,
    score: Option<String>,
    targets: Option<BTreeMap<String, f64>>,
    spend: Option<String>,
    budget: Option<f64>,
    halt: Option<String>,
    protect: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScoreConfig {
    pub(crate) command: Option<String>,
    pub(crate) targets: BTreeMap<String, f64>,
    pub(crate) spend: Option<String>,
    pub(crate) budget: Option<f64>,
    pub(crate) halt: Option<String>,
}

#[must_use]
pub(crate) fn check_path(workspace: &Path) -> PathBuf {
    workspace.join(".summonr/check.toml")
}

fn invalid(path: &Path, why: &str) -> Error {
    Error::Config(format!("{}: {why}", path.display()))
}

fn parse_file(path: &Path, text: &str) -> Result<CheckFile> {
    let file: CheckFile =
        toml::from_str(text).map_err(|error| invalid(path, &error.to_string()))?;
    let commands = [
        ("score", &file.score),
        ("spend", &file.spend),
        ("halt", &file.halt),
    ];
    for (key, command) in commands {
        if command
            .as_ref()
            .is_some_and(|command| command.trim().is_empty())
        {
            return Err(invalid(path, &format!("`{key}` must name a command")));
        }
    }
    if file.targets.is_some() && file.score.is_none() {
        return Err(invalid(path, "`targets` needs a `score` command"));
    }
    if file.budget.is_some() && file.spend.is_none() {
        return Err(invalid(path, "`budget` needs a `spend` command"));
    }
    if file
        .budget
        .is_some_and(|budget| !budget.is_finite() || budget < 0.0)
    {
        return Err(invalid(path, "`budget` must be a number of dollars"));
    }
    if file.halt.is_some() && file.budget.is_none() {
        return Err(invalid(path, "`halt` needs a `budget`"));
    }
    Ok(file)
}

fn check_file(workspace: &Path) -> Result<Option<(PathBuf, CheckFile)>> {
    let path = check_path(workspace);
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    let file = parse_file(&path, &text)?;
    Ok(Some((path, file)))
}

pub fn check_commands(workspace: &Path) -> Result<Option<Vec<String>>> {
    let Some((path, file)) = check_file(workspace)? else {
        return Ok(None);
    };
    if file.check.is_some() && file.checks.is_some() {
        return Err(invalid(&path, "use `check` or `checks`, not both"));
    }
    let scored = file.score.is_some();
    let mut commands = match (file.check, file.checks) {
        (Some(check), _) => vec![check],
        (None, Some(checks)) => checks,
        (None, None) if scored => return Ok(None),
        (None, None) => Vec::new(),
    };
    commands.retain(|command| !command.trim().is_empty());
    if commands.is_empty() {
        return Err(invalid(&path, "`check` must name at least one command"));
    }
    Ok(Some(commands))
}

pub(crate) fn score_config(workspace: &Path, bytes: &[u8]) -> Result<Option<ScoreConfig>> {
    let path = check_path(workspace);
    let text = std::str::from_utf8(bytes).map_err(|error| invalid(&path, &error.to_string()))?;
    let file = parse_file(&path, text)?;
    if file.score.is_none() && file.spend.is_none() {
        return Ok(None);
    }
    Ok(Some(ScoreConfig {
        command: file.score,
        targets: file.targets.unwrap_or_default(),
        spend: file.spend,
        budget: file.budget,
        halt: file.halt,
    }))
}

pub(crate) fn protected(workspace: &Path, bytes: &[u8]) -> BTreeSet<PathBuf> {
    let path = check_path(workspace);
    let Ok(file) = std::str::from_utf8(bytes)
        .map_err(|error| invalid(&path, &error.to_string()))
        .and_then(|text| parse_file(&path, text))
    else {
        return BTreeSet::new();
    };
    let measures = [&file.score, &file.spend, &file.halt]
        .into_iter()
        .flatten()
        .flat_map(|command| command.split_whitespace());
    let programs = file
        .check
        .iter()
        .chain(file.checks.iter().flatten())
        .flat_map(|command| command.split(['&', '|', ';']))
        .filter_map(|segment| segment.split_whitespace().next());
    let mut paths: BTreeSet<PathBuf> = measures
        .chain(programs)
        .map(|token| token.trim_matches(['"', '\'']))
        .filter(|token| !token.starts_with('/') && !token.contains(".."))
        .map(|token| workspace.join(token))
        .filter(|path| path.is_file())
        .collect();
    for entry in file.protect.iter().flatten() {
        let walk = ignore::WalkBuilder::new(workspace.join(entry))
            .hidden(false)
            .build();
        paths.extend(
            walk.flatten()
                .map(ignore::DirEntry::into_path)
                .filter(|path| path.is_file()),
        );
    }
    paths
}
