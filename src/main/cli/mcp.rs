use std::fmt;

use summonr::table::{Row, Table};
use summonr::{Error, McpLaunch, Result};

use super::flags::Input;

#[derive(Clone)]
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
        "-h" | "--help" => super::exit_help(Some("mcp")),
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
    if input
        .peek()
        .is_some_and(|next| next == "-h" || next == "--help")
    {
        super::exit_help(Some("mcp"));
    }
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
    match input.next().as_deref() {
        None => Ok(()),
        Some("-h" | "--help") => super::exit_help(Some("mcp")),
        Some(extra) => Err(usage(&format!("unexpected `{extra}`; {USAGE}"))),
    }
}

fn add(input: &mut Input) -> Result<McpCommand> {
    let name = name(input).map_err(|_unnamed| usage(ADD_USAGE))?;
    let mut flags = AddFlags::default();
    ADD.walk(&mut flags, input.by_ref())?;
    let launch = flags.launch()?;
    Ok(McpCommand::Add { name, launch })
}

#[derive(Default)]
struct AddFlags {
    env: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    url: Option<String>,
    command: Vec<String>,
}

type AddFlag = Row<AddFlags>;

const ADD: Table<AddFlags> = Table {
    rows: &[
        AddFlag::value(&["--env"], "KEY=VALUE", |flags, raw| {
            flags.env.push(pair(&raw)?);
            Ok(())
        }),
        AddFlag::value(&["--header"], "KEY=VALUE", |flags, raw| {
            flags.headers.push(pair(&raw)?);
            Ok(())
        }),
        AddFlag::value(&["--url"], "a URL", |flags, url| {
            flags.url = Some(url);
            Ok(())
        })
        .once(),
        AddFlag::rest(&["--"], |flags, command| flags.command.extend(command)),
        AddFlag::switch(&["-h", "--help"], |_| super::exit_help(Some("mcp"))),
    ],
    missing: |flag, expected| format!("{flag} requires {expected}"),
    unknown: |flag| format!("usage: unexpected `{flag}`; {ADD_USAGE}"),
    error: Error::Config,
    dash_values: false,
    pairs: false,
};

impl AddFlags {
    fn launch(self) -> Result<McpLaunch> {
        let Self {
            env,
            headers,
            url,
            command,
        } = self;
        let advice = match (url, command.split_first()) {
            (Some(url), None) if env.is_empty() => {
                return Ok(McpLaunch::Http {
                    url,
                    headers,
                    auth: None,
                });
            }
            (None, Some((command, args))) if headers.is_empty() => {
                return Ok(McpLaunch::Stdio {
                    command: command.clone(),
                    args: args.to_vec(),
                    env,
                });
            }
            (None, None) => "give a command after `--` or a server with `--url URL`",
            (Some(_), Some(_)) => "give a command after `--` or `--url`, not both",
            (Some(_), None) => {
                "`--env` sets variables for a command server; use `--header` with `--url`"
            }
            (None, Some(_)) => {
                "`--header` works only with `--url`; use `--env` for a command server"
            }
        };
        Err(usage(&format!("{advice}; {ADD_USAGE}")))
    }
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
