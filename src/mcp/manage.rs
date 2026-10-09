use toml_edit::{Array, InlineTable, Item, Table, value};

use crate::config;
use crate::error::{Error, Result};

#[derive(Clone)]
pub enum McpLaunch {
    Command {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    Url {
        url: String,
        headers: Vec<(String, String)>,
    },
}

pub fn add(name: &str, launch: &McpLaunch) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(Error::Config(format!(
            "mcp name `{name}`: letters, digits, `-`, and `_` only"
        )));
    }
    let mut table = Table::new();
    match launch {
        McpLaunch::Command { command, args, env } => {
            table["command"] = value(command.as_str());
            if !args.is_empty() {
                table["args"] = value(args.iter().map(String::as_str).collect::<Array>());
            }
            if !env.is_empty() {
                table["env"] = value(pairs(env));
            }
        }
        McpLaunch::Url { url, headers } => {
            table["url"] = value(url.as_str());
            if !headers.is_empty() {
                table["headers"] = value(pairs(headers));
            }
        }
    }
    if config::save_named(
        &config::path()?,
        config::Section::Mcp,
        name,
        Item::Table(table),
    )? {
        return Ok(());
    }
    Err(Error::Config(format!(
        "mcp `{name}` already in config.toml; remove it first"
    )))
}

pub fn remove(name: &str) -> Result<bool> {
    config::remove_named(&config::path()?, config::Section::Mcp, name)
}

fn pairs(pairs: &[(String, String)]) -> InlineTable {
    pairs
        .iter()
        .map(|(key, entry)| (key.as_str(), entry.as_str()))
        .collect()
}
