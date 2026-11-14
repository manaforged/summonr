use std::fmt;

use summonr_agent::table::{self, Row, Table};
use summonr_agent::{Error, McpLaunch, Result};

use super::commands::Command;
use super::flags::{Input, verb};
use super::{Args, Verb};

#[derive(Clone)]
pub(crate) enum McpCommand {
    List,
    Get { name: String },
    Add { name: String, launch: McpLaunch },
    Remove { name: String },
}

impl fmt::Debug for McpCommand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List => write!(formatter, "List"),
            Self::Get { name } => write!(formatter, "Get {{ name: {name} }}"),
            Self::Add { name, .. } => write!(formatter, "Add {{ name: {name} }}"),
            Self::Remove { name } => write!(formatter, "Remove {{ name: {name} }}"),
        }
    }
}

const ADD_USAGE: &str = "summonr mcp add NAME [--env KEY=VALUE]... -- COMMAND [ARGS...]  or  summonr mcp add NAME --url URL [--header KEY=VALUE]...";

pub(super) const ROWS: [Command; 4] = [
    Command::hidden("mcp list", |args, _| mcp(args, McpCommand::List)),
    Command::hidden("mcp get", |args, input| {
        let name = args.word(input, "NAME")?;
        mcp(args, McpCommand::Get { name })
    }),
    Command::hidden("mcp add", add),
    Command::hidden("mcp remove", |args, input| {
        let name = args.word(input, "NAME")?;
        mcp(args, McpCommand::Remove { name })
    }),
];

fn mcp(args: &mut Args, command: McpCommand) -> Result<()> {
    verb(args, Verb::Mcp(command))
}

fn add(args: &mut Args, input: &mut Input) -> Result<()> {
    let name = args
        .word(input, "NAME")
        .map_err(|_unnamed| usage(ADD_USAGE))?;
    let mut flags = AddFlags::default();
    ADD.walk(&mut flags, input.by_ref())?;
    let launch = flags.launch()?;
    mcp(args, McpCommand::Add { name, launch })
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
            flags.env.push(summonr_agent::mcp::pair(&raw)?);
            Ok(())
        }),
        AddFlag::value(&["--header"], "KEY=VALUE", |flags, raw| {
            flags.headers.push(summonr_agent::mcp::pair(&raw)?);
            Ok(())
        }),
        AddFlag::value(&["--url"], "a URL", |flags, url| {
            flags.url = Some(url);
            Ok(())
        })
        .once(),
        AddFlag::rest(&["--"], |flags, command| flags.command.extend(command)),
    ],
    missing: table::requires,
    unknown: |flag| format!("usage: unexpected `{flag}`; {ADD_USAGE}"),
    error: Error::Setup,
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

fn usage(text: &str) -> Error {
    Error::Setup(format!("usage: {text}"))
}
