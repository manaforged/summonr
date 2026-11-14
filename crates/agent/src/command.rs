use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;
use serde::de::IntoDeserializer;
use serde::de::value::{Error, StrDeserializer};

use Argument::{Bare, Text, Value};

const PROMPT_PREFIX: &str = "mcp__";
const PROMPT_SEPARATOR: &str = "__";

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
    Stop,
    Auto,
    Goal,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub command: SlashCommand,
    pub argument: Argument,
    pub group: &'static str,
    pub developer: bool,
    pub usage: &'static str,
}

impl CommandSpec {
    #[must_use]
    pub fn shown(&self) -> bool {
        !self.developer || std::env::var_os("SUMMONR_DEV").is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Argument {
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
        let data: Data = summonr_core::embedded!("command", "command/commands.toml");
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
/// Returns the shown command nearest to a mistyped one: a shared prefix first, then the same letters in order.
pub(crate) fn closest(typed: &str) -> Option<&'static str> {
    let needle = typed.trim_start_matches('/').to_lowercase();
    let shown = || {
        command_specs()
            .iter()
            .filter(|spec| spec.shown())
            .map(|spec| spec.name)
    };
    let bare = |name: &'static str| name.trim_start_matches('/');
    shown()
        .filter(|name| bare(name).starts_with(&needle))
        .min()
        .or_else(|| {
            shown()
                .filter(|name| {
                    let mut letters = bare(name).chars();
                    needle
                        .chars()
                        .all(|letter| letters.any(|found| found == letter))
                })
                .min()
        })
}

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
        .is_some_and(|head| prompt_head(head).is_some())
}

/// How a typed command head names a server prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromptHead<'a> {
    Named { server: &'a str, prompt: &'a str },
    Joined(&'a str),
}

/// Reads `/server:prompt`, or `/mcp__server__prompt` with the server and prompt still joined.
#[must_use]
pub(crate) fn prompt_head(head: &str) -> Option<PromptHead<'_>> {
    let name = head.strip_prefix('/')?;
    if let Some(joined) = name.strip_prefix(PROMPT_PREFIX) {
        return joined
            .contains(PROMPT_SEPARATOR)
            .then_some(PromptHead::Joined(joined));
    }
    let (server, prompt) = name.split_once(':')?;
    Some(PromptHead::Named { server, prompt })
}

/// The `/mcp__server__prompt` spelling of a server name: characters outside `A-Z`, `a-z`, `0-9`, `_`, `-` become `_`.
#[must_use]
pub(crate) fn prompt_server(server: &str) -> String {
    summonr_core::text::clean_name(server, "_-", '_', usize::MAX)
}

/// The prompt that `joined` names after `server`'s spelling and the separator.
#[must_use]
pub(crate) fn joined_prompt<'a>(joined: &'a str, server: &str) -> Option<&'a str> {
    joined
        .strip_prefix(prompt_server(server).as_str())?
        .strip_prefix(PROMPT_SEPARATOR)
        .filter(|prompt| !prompt.is_empty())
}

pub fn alias(line: &str) -> Option<String> {
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

pub fn argument<'a>(line: &'a str, workspace: &std::path::Path) -> Result<Option<&'a str>, String> {
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
