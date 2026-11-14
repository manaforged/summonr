use summonr_agent::cron::cli::{Command as Cron, Show, Spec};
use summonr_agent::{Error, Result};

use super::commands::Command;
use super::flags::{Input, verb};
use super::{Args, Verb};

pub(super) const ROWS: [Command; 15] = [
    Command::hidden("cron add", |args, input| {
        let name = args.word(input, "NAME")?;
        cron(args, Cron::Add(name, Spec::parse(input.by_ref())?))
    }),
    Command::hidden("cron edit", |args, input| {
        let name = args.word(input, "NAME")?;
        cron(args, Cron::Edit(name, Spec::parse(input.by_ref())?))
    }),
    Command::hidden("cron list", |args, _| cron(args, Cron::List)),
    Command::hidden("cron show", |args, input| named(args, input, Cron::Show)),
    Command::hidden("cron pause", |args, input| named(args, input, Cron::Pause)),
    Command::hidden("cron resume", |args, input| {
        named(args, input, Cron::Resume)
    }),
    Command::hidden("cron run", |args, input| {
        let name = args.word(input, "NAME")?;
        cron(args, Cron::Run(name, Show::parse(input.by_ref(), false)?))
    }),
    Command::hidden("cron remove", |args, input| {
        named(args, input, Cron::Remove)
    }),
    Command::hidden("cron log", |args, input| {
        let name = args.word(input, "NAME")?;
        cron(args, Cron::Log(name, Show::parse(input.by_ref(), true)?))
    }),
    Command::hidden("cron token", |args, input| named(args, input, Cron::Token)),
    Command::hidden("cron install", |args, _| cron(args, Cron::Install)),
    Command::hidden("cron uninstall", |args, _| cron(args, Cron::Uninstall)),
    Command::hidden("cron status", |args, _| cron(args, Cron::Status)),
    Command::hidden("cron tick", |args, _| cron(args, Cron::Tick)),
    Command::hidden("cron exec", exec),
];

fn cron(args: &mut Args, command: Cron) -> Result<()> {
    verb(args, Verb::Cron(Box::new(command)))
}

fn named(args: &mut Args, input: &mut Input, command: fn(String) -> Cron) -> Result<()> {
    let name = args.word(input, "NAME")?;
    cron(args, command(name))
}

fn exec(args: &mut Args, input: &mut Input) -> Result<()> {
    let name = args.word(input, "NAME")?;
    let usage = || Error::Setup("exec takes NAME --slot N [--payload FILE]".to_string());
    input.next_if(|word| word == "--slot").ok_or_else(usage)?;
    let slot =
        summonr_agent::table::number(&input.next().ok_or_else(usage)?).map_err(Error::Setup)?;
    let payload = match input.next_if(|word| word == "--payload") {
        Some(_) => Some(input.next().ok_or_else(usage)?.into()),
        None => None,
    };
    cron(
        args,
        Cron::Exec {
            name,
            slot,
            payload,
        },
    )
}
