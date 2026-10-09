use std::sync::atomic::{AtomicU64, Ordering};

use Argument::{Bare, Text, Value};
use Category::{Developer, Files, Model, Session, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Category {
    Session,
    Model,
    Files,
    Settings,
    Developer,
}

impl Category {
    pub(crate) const ALL: [Self; 5] = [Session, Model, Files, Settings, Developer];

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Session => "conversation and sessions",
            Model => "model and mode",
            Files => "files and panes",
            Settings => "settings and permissions",
            Developer => "developer",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub(crate) category: Category,
    pub(crate) usage: &'static str,
}

impl CommandSpec {
    const fn usage(mut self, usage: &'static str) -> Self {
        self.usage = usage;
        self
    }

    #[must_use]
    pub fn shown(&self) -> bool {
        self.category != Developer || std::env::var_os("SUMMONR_DEV").is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Argument {
    Bare,
    Value,
    Text,
}

mod specs;

use specs::{ALIASES, SPECS};

static NOTED: AtomicU64 = AtomicU64::new(0);

#[must_use]
pub const fn command_specs() -> &'static [CommandSpec] {
    SPECS
}

#[must_use]
pub fn parse_command(line: &str) -> Option<SlashCommand> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if !tokens.first().is_some_and(|head| head.starts_with('/')) || server_prompt(line) {
        return None;
    }
    (1..=tokens.len().min(2))
        .rev()
        .find_map(|count| {
            let head = tokens[..count].join(" ");
            SPECS
                .iter()
                .find(|spec| spec.name == head && spec.shown())
                .map(|spec| spec.command)
        })
        .or_else(|| {
            ALIASES
                .iter()
                .find(|(name, _)| *name == tokens[0])
                .and_then(|(_, target)| parse_command(target))
        })
}

#[must_use]
pub(crate) fn server_prompt(line: &str) -> bool {
    line.split_whitespace()
        .next()
        .and_then(|head| head.strip_prefix('/'))
        .is_some_and(|name| name.contains(':'))
}

pub(crate) fn renamed(line: &str) -> Option<(String, Option<String>)> {
    let line = line.trim();
    let (head, tail) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let index = ALIASES.iter().position(|(name, _)| *name == head)?;
    let target = ALIASES[index].1;
    let bare = SPECS
        .iter()
        .any(|spec| spec.name == target && spec.argument == Bare);
    let expanded = if bare || tail.trim().is_empty() {
        target.to_string()
    } else {
        format!("{target} {}", tail.trim())
    };
    let bit = 1_u64 << index;
    let first = NOTED.fetch_or(bit, Ordering::Relaxed) & bit == 0;
    Some((expanded, first.then(|| format!("renamed: use {target}"))))
}

pub(crate) fn argument(line: &str) -> Result<Option<&str>, String> {
    let Some(command) = parse_command(line) else {
        if let Some(head) = line.split_whitespace().next().filter(|head| {
            head.starts_with('/') && !server_prompt(head) && !crate::plugins::commands::named(head)
        }) {
            return Err(format!(
                "Unknown command `{head}`. Use /help or press Tab to complete."
            ));
        }
        return Ok(None);
    };
    let Some(spec) = SPECS.iter().find(|spec| spec.command == command) else {
        return Ok(None);
    };
    let mut tail = line.trim();
    for _ in spec.name.split_whitespace() {
        tail = tail
            .split_once(char::is_whitespace)
            .map_or("", |(_, tail)| tail.trim());
    }
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
