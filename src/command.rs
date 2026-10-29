use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;
use serde::de::IntoDeserializer;
use serde::de::value::{Error, StrDeserializer};

use Argument::{Bare, Text, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlashCommand {
    Help,
    Settings,
    Login,
    Model,
    Effort,
    Workflow,
    Status,
    Mcp,
    Resume,
    New,
    Search,
    History,
    Retry,
    Compact,
    Name,
    Steps,
    Pane,
    Diff,
    Copy,
    Export,
    View,
    Agents,
    Memory,
    Skills,
    Permissions,
    Fork,
    Usage,
    Demo,
    Undo,
    Clear,
    Auto,
    Goal,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub command: SlashCommand,
    argument: Argument,
    pub(crate) group: &'static str,
    developer: bool,
    pub(crate) usage: &'static str,
}

impl CommandSpec {
    #[must_use]
    pub fn shown(&self) -> bool {
        !self.developer || std::env::var_os("SUMMONR_DEV").is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Argument {
    Bare,
    Value,
    Text,
}

#[derive(Deserialize)]
struct Data {
    retired: BTreeMap<String, String>,
    group: Vec<Group>,
}

#[derive(Deserialize)]
struct Group {
    title: String,
    #[serde(default)]
    developer: bool,
    commands: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    name: String,
    takes: Argument,
    #[serde(default)]
    usage: String,
    help: String,
}

struct Commands {
    specs: Vec<CommandSpec>,
    retired: Vec<(&'static str, &'static str)>,
}

pub mod table;

fn commands() -> &'static Commands {
    static LOADED: OnceLock<Commands> = OnceLock::new();
    LOADED.get_or_init(|| {
        let data: Data =
            summonr_core::table::embedded("command", include_str!("command/commands.toml"));
        let mut specs = Vec::new();
        for group in data.group {
            let title: &'static str = group.title.leak();
            for row in group.commands {
                let key: StrDeserializer<'_, Error> =
                    row.name.trim_start_matches('/').into_deserializer();
                let command = match SlashCommand::deserialize(key) {
                    Ok(command) => command,
                    Err(error) => panic!("the built-in command table is invalid: {error}"),
                };
                specs.push(CommandSpec {
                    name: row.name.leak(),
                    description: row.help.leak(),
                    command,
                    argument: row.takes,
                    group: title,
                    developer: group.developer,
                    usage: row.usage.leak(),
                });
            }
        }
        let retired = data
            .retired
            .into_iter()
            .map(|(name, target)| -> (&'static str, &'static str) { (name.leak(), target.leak()) })
            .collect();
        Commands { specs, retired }
    })
}

#[must_use]
pub fn command_specs() -> &'static [CommandSpec] {
    &commands().specs
}

fn retired(head: &str) -> Option<&'static str> {
    commands()
        .retired
        .iter()
        .find(|(name, _)| *name == head)
        .map(|(_, target)| *target)
}

#[must_use]
pub fn parse_command(line: &str) -> Option<SlashCommand> {
    let head = line.split_whitespace().next()?;
    if !head.starts_with('/') || server_prompt(line) {
        return None;
    }
    command_specs()
        .iter()
        .find(|spec| spec.name == head && spec.shown())
        .map(|spec| spec.command)
        .or_else(|| retired(head).and_then(parse_command))
}

#[must_use]
pub(crate) fn server_prompt(line: &str) -> bool {
    line.split_whitespace()
        .next()
        .and_then(|head| head.strip_prefix('/'))
        .is_some_and(|name| name.contains(':'))
}

pub(crate) fn alias(line: &str) -> Option<String> {
    let line = line.trim();
    let (head, tail) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let target = retired(head)?;
    let bare = command_specs()
        .iter()
        .any(|spec| spec.name == target && spec.argument == Bare);
    Some(if bare || tail.trim().is_empty() {
        target.to_string()
    } else {
        format!("{target} {}", tail.trim())
    })
}

pub(crate) fn argument<'a>(
    line: &'a str,
    workspace: &std::path::Path,
) -> Result<Option<&'a str>, String> {
    let Some(command) = parse_command(line) else {
        if let Some(head) = line.split_whitespace().next().filter(|head| {
            head.starts_with('/')
                && !server_prompt(head)
                && !crate::plugins::commands::named(head, workspace)
                && !crate::skills::list(workspace)
                    .iter()
                    .any(|(name, _)| head.strip_prefix('/') == Some(name.as_str()))
        }) {
            return Err(format!(
                "Unknown command `{head}`. Use /help or press Tab to complete."
            ));
        }
        return Ok(None);
    };
    let Some(spec) = command_specs().iter().find(|spec| spec.command == command) else {
        return Ok(None);
    };
    let mut tail = line
        .trim()
        .split_once(char::is_whitespace)
        .map_or("", |(_, tail)| tail.trim());
    if tail.is_empty() {
        return Ok(None);
    }
    match spec.argument {
        Bare => Err(format!("{} takes no arguments.", spec.name)),
        Text => Ok(Some(tail)),
        Value => {
            if let Some(quote) = tail
                .chars()
                .next()
                .filter(|quote| matches!(quote, '\'' | '"'))
            {
                tail = tail
                    .strip_prefix(quote)
                    .and_then(|tail| tail.strip_suffix(quote))
                    .ok_or_else(|| format!("Close the quoted argument for {}.", spec.name))?;
            }
            Ok((!tail.is_empty()).then_some(tail))
        }
    }
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
