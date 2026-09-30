use std::fmt;

use summonr::{Error, McpLaunch, Result};

use super::flags::Input;
use super::value;

pub(crate) enum McpCommand {
    List { json: bool },
    Get { name: String, json: bool },
    Add { name: String, launch: McpLaunch },
    Remove { name: String },
}

impl fmt::Debug for McpCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List { json } => write!(formatter, "List {{ json: {json} }}"),
            Self::Get { name, json } => write!(formatter, "Get {{ name: {name}, json: {json} }}"),
            Self::Add { name, .. } => write!(formatter, "Add {{ name: {name} }}"),
            Self::Remove { name } => write!(formatter, "Remove {{ name: {name} }}"),
        }
    }
}

const USAGE: &str = "summonr mcp list|get NAME|add NAME ...|remove NAME";
const ADD_USAGE: &str = "summonr mcp add NAME [--env KEY=VALUE]... -- COMMAND [ARGS...]  or  summonr mcp add NAME --url URL [--header KEY=VALUE]...";

pub(super) fn parse(input: &mut Input) -> Result<McpCommand> {
    let verb = input.next().ok_or_else(|| usage(USAGE))?;
    match verb.as_str() {
        "list" => Ok(McpCommand::List { json: json(input)? }),
        "get" => {
            let name = name(input)?;
            Ok(McpCommand::Get {
                name,
                json: json(input)?,
            })
        }
        "add" => add(input),
        "remove" => {
            let name = name(input)?;
            finished(input)?;
            Ok(McpCommand::Remove { name })
        }
        unknown => Err(usage(&format!("unknown `mcp {unknown}`; {USAGE}"))),
    }
}

fn name(input: &mut Input) -> Result<String> {
    input
        .next_if(|next| !next.starts_with('-'))
        .ok_or_else(|| usage(USAGE))
}

fn json(input: &mut Input) -> Result<bool> {
    let json = input.next_if(|next| next == "--json").is_some();
    finished(input)?;
    Ok(json)
}

fn finished(input: &mut Input) -> Result<()> {
    input.next().map_or(Ok(()), |extra| {
        Err(usage(&format!("unexpected `{extra}`; {USAGE}")))
    })
}

fn add(input: &mut Input) -> Result<McpCommand> {
    let name = input
        .next_if(|next| !next.starts_with('-'))
        .ok_or_else(|| usage(ADD_USAGE))?;
    let mut env = Vec::new();
    let mut headers = Vec::new();
    let mut url = None;
    let mut command = Vec::new();
    while let Some(arg) = input.next() {
        match arg.as_str() {
            "--env" => env.push(pair(&value(input, "--env", "KEY=VALUE")?)?),
            "--header" => headers.push(pair(&value(input, "--header", "KEY=VALUE")?)?),
            "--url" if url.is_none() => url = Some(value(input, "--url", "a URL")?),
            "--" => command.extend(input.by_ref()),
            unknown => return Err(usage(&format!("unexpected `{unknown}`; {ADD_USAGE}"))),
        }
    }
    let launch = match (url, command.split_first()) {
        (Some(url), None) if env.is_empty() => McpLaunch::Url { url, headers },
        (None, Some((command, args))) if headers.is_empty() => McpLaunch::Command {
            command: command.clone(),
            args: args.to_vec(),
            env,
        },
        (Some(_) | None, Some(_) | None) => {
            return Err(usage(&format!(
                "give a command after `--` or `--url`, not both; {ADD_USAGE}"
            )));
        }
    };
    Ok(McpCommand::Add { name, launch })
}

fn pair(raw: &str) -> Result<(String, String)> {
    raw.split_once('=')
        .filter(|(key, _)| !key.is_empty())
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .ok_or_else(|| Error::Config(format!("expected KEY=VALUE, got `{raw}`")))
}

fn usage(text: &str) -> Error {
    Error::Config(format!("usage: {text}"))
}
