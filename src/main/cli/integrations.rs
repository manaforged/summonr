use summonr_agent::gateway::cli::{Command as Gateway, Options};
use summonr_agent::mail::cli::Command as Mail;
use summonr_agent::panel::cli::Command as Panel;
use summonr_agent::table::{self, Arity, Row, Table};
use summonr_agent::{Error, Result};

use super::commands::Command;
use super::flags::{Input, misuse, verb};
use super::{Args, Verb};

pub(super) const MAIL: [Command; 4] = [
    Command::hidden("mail agents", |args, _| mail(args, Mail::Agents)),
    Command::hidden("mail send", send),
    Command::hidden("mail inbox", |args, input| {
        let id = args.word(input, "ID")?;
        let clear = input.next_if(|word| word == "--clear").is_some();
        mail(args, Mail::Inbox { id, clear })
    }),
    Command::hidden("mail board", board),
];

pub(super) const GATEWAY: [Command; 6] = [
    Command::hidden("gateway setup", |args, input| {
        input
            .next_if(|word| word == "telegram")
            .ok_or_else(|| misuse("gateway", "`summonr gateway setup` takes `telegram`"))?;
        gateway(args, Gateway::Setup)
    }),
    Command::hidden("gateway telegram", |args, input| {
        let options = Options::parse(input.by_ref(), &std::env::current_dir()?)?;
        gateway(args, Gateway::Telegram(options))
    }),
    Command::hidden("gateway pair", |args, input| {
        let code = args.word(input, "the code the bot sent you")?;
        gateway(args, Gateway::Pair(code))
    }),
    Command::hidden("gateway unpair", |args, input| {
        let user = args.word(input, "USER_ID")?.parse().ok().ok_or_else(|| {
            Error::Setup(
                "USER_ID is a number; `summonr gateway status` lists paired users".to_string(),
            )
        })?;
        gateway(args, Gateway::Unpair(user))
    }),
    Command::hidden("gateway status", |args, _| gateway(args, Gateway::Status)),
    Command::hidden("gateway webhook", |args, input| {
        let port = match input.next_if(|word| word == "--port") {
            Some(flag) => Some(port(input, &flag)?),
            None => None,
        };
        gateway(args, Gateway::Webhook(port))
    }),
];

pub(super) const PANEL: [Command; 2] = [
    Command::hidden("panel list", |args, _| verb(args, Verb::Panel(Panel::List))),
    Command::hidden("panel check", |args, input| {
        let name = args.word(input, "NAME")?;
        verb(args, Verb::Panel(Panel::Check(name)))
    }),
];

pub(super) fn tool(args: &mut Args, input: &mut Input) -> Result<()> {
    input.next_if(|word| word == "run");
    let name = input.next().ok_or_else(|| {
        Error::Setup("summonr tool run NAME [JSON]: invoke a tool directly".to_string())
    })?;
    let rest: Vec<String> = input.by_ref().collect();
    let arguments = if rest.is_empty() {
        "{}".to_string()
    } else {
        rest.join(" ")
    };
    verb(args, Verb::Tool { name, arguments })
}

fn mail(args: &mut Args, command: Mail) -> Result<()> {
    verb(args, Verb::Mail(command))
}

fn gateway(args: &mut Args, command: Gateway) -> Result<()> {
    verb(args, Verb::Gateway(command))
}

fn port(input: &mut Input, flag: &str) -> Result<u16> {
    let value = input
        .next()
        .ok_or_else(|| misuse("gateway", &table::needs_value(flag, "")))?;
    summonr_agent::table::number(&value).map_err(|message| misuse("gateway", &message))
}

#[derive(Default)]
struct SendFlags {
    json: bool,
    reply_to: Option<String>,
}

const SEND: Table<SendFlags> = Table {
    rows: &[
        Row::switch(&["--json"], |flags| {
            flags.json = true;
            Ok(())
        }),
        Row::value(&["--reply-to"], "a message id", |flags, id| {
            flags.reply_to = Some(id);
            Ok(())
        }),
    ],
    missing: table::requires,
    unknown: |flag| format!("unknown mail send option {flag}"),
    error: Error::Setup,
    dash_values: false,
    pairs: false,
};

pub(super) fn send_value(word: &str) -> bool {
    SEND.find(word, &SendFlags::default())
        .is_some_and(|row| matches!(row.arity, Arity::Value(..)))
}

fn send(args: &mut Args, input: &mut Input) -> Result<()> {
    let mut flags = SendFlags::default();
    SEND.lead(&mut flags, input)?;
    args.json |= flags.json;
    let needs = || misuse("mail", "`summonr mail send` needs <name|id|all> <text>");
    let to = input.next().ok_or_else(needs)?;
    let text = input.by_ref().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(needs());
    }
    mail(
        args,
        Mail::Send {
            to,
            text,
            reply_to: flags.reply_to,
        },
    )
}

fn board(args: &mut Args, input: &mut Input) -> Result<()> {
    let command = match input.next_if(|word| !word.starts_with('-')).as_deref() {
        None | Some("list") => Mail::Board,
        Some("get") => Mail::BoardGet(args.word(input, "KEY")?),
        Some("set") => {
            let key = args.word(input, "KEY")?;
            let text = input.by_ref().collect::<Vec<_>>().join(" ");
            if text.is_empty() {
                return Err(misuse("mail", "`summonr mail board set` needs KEY TEXT"));
            }
            Mail::BoardSet { key, text }
        }
        Some(other) => {
            return Err(misuse(
                "mail",
                &format!("`summonr mail board` takes list, get, or set; got `{other}`"),
            ));
        }
    };
    mail(args, command)
}
