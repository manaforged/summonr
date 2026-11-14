use std::path::Path;

use super::telegram::Bot;
use super::turn;
pub use super::turn::Options;
use super::{SECRET, block, store, token};
use crate::command::table::{self, Row, Table};
use summonr_core::error::{Error, Result};

/// The `summonr gateway --help` text.
pub const USAGE: &str = "usage:\n  summonr gateway setup telegram\n  summonr gateway telegram [--endpoint E] [--model M] [--dir D]\n  summonr gateway pair CODE\n  summonr gateway unpair USER_ID\n  summonr gateway status\n  summonr gateway webhook [--port N]\n\nThe Telegram bridge long-polls the Bot API; no port opens.";
const TOKEN_PROMPT: &str = "Telegram bot token: ";
const NO_TOKEN: &str = "no Telegram bot token stored: run `summonr gateway setup telegram`";

/// One `summonr gateway` command.
#[derive(Clone, Debug)]
pub enum Command {
    /// `gateway setup telegram`: store the bot token.
    Setup,
    /// `gateway telegram`: run the bridge.
    Telegram(Options),
    /// `gateway pair CODE`.
    Pair(String),
    /// `gateway unpair USER_ID`.
    Unpair(i64),
    /// `gateway status`.
    Status,
    /// `gateway webhook [--port N]`; no port means the configured one.
    Webhook(Option<u16>),
}

/// Runs one `summonr gateway` command.
pub fn run(command: &Command, read_secret: &dyn Fn(&str) -> Result<String>) -> Result<i32> {
    match command {
        Command::Setup => setup(read_secret),
        Command::Telegram(options) => bridge(options.clone()),
        Command::Pair(code) => pair(code),
        Command::Unpair(user) => unpair(*user),
        Command::Status => status(),
        Command::Webhook(port) => {
            super::webhook::serve(port.unwrap_or(crate::limits::limits().gateway_port))
        }
    }
}

fn misuse(message: String) -> Error {
    table::misuse("gateway", &message)
}

type OptionFlag = Row<Options>;

const OPTIONS: Table<Options> = Table {
    rows: &[
        OptionFlag::value(&["--endpoint"], "", |options, value| {
            options.endpoint = Some(value);
            Ok(())
        }),
        OptionFlag::value(&["--model"], "", |options, value| {
            options.model = Some(value);
            Ok(())
        }),
        OptionFlag::value(&["--dir"], "", |options, value| {
            options.dir = value.into();
            Ok(())
        }),
    ],
    missing: table::needs_value,
    unknown: table::unknown_option,
    error: misuse,
    dash_values: true,
    pairs: true,
};

impl Options {
    /// Reads the flags of `gateway telegram`; `--dir` is relative to `cwd`.
    pub fn parse(words: impl IntoIterator<Item = String>, cwd: &Path) -> Result<Self> {
        let mut options = Self {
            dir: cwd.to_path_buf(),
            ..Self::default()
        };
        OPTIONS.walk(&mut options, words)?;
        options.dir = cwd.join(&options.dir);
        Ok(options)
    }
}

fn setup(read_secret: &dyn Fn(&str) -> Result<String>) -> Result<i32> {
    let token = read_secret(TOKEN_PROMPT)?.trim().to_string();
    if token.is_empty() {
        return Err(Error::Config("no token given".to_string()));
    }
    let bot = Bot::new(&token)?;
    let me = block(bot.me())??;
    crate::secret::set(SECRET, &token)?;
    println!(
        "stored the Telegram bot token for @{}",
        me.username.as_deref().unwrap_or("?")
    );
    Ok(0)
}

fn bridge(options: Options) -> Result<i32> {
    let token = token()?.ok_or_else(|| Error::Config(NO_TOKEN.to_string()))?;
    let bot = Bot::new(&token)?;
    let me = block(bot.me())??;
    println!(
        "@{} is listening; chats work in {}. Ctrl+C stops.",
        me.username.as_deref().unwrap_or("?"),
        crate::config::tilde(&options.dir)
    );
    if let Err(error) = block(bot.set_commands(&super::menu()))? {
        eprintln!("{error}");
    }
    block(turn::serve(bot, options, me.username))?
}

fn pair(code: &str) -> Result<i32> {
    let now = crate::cron::now();
    let pending = store::update(|state| state.pair(code, now))?
        .ok_or_else(|| Error::Setup(format!("no pending pairing with code `{code}`")))?;
    println!("paired Telegram user {}", pending.user);
    if let Some(token) = token()? {
        let bot = Bot::new(&token)?;
        drop(block(bot.send(pending.chat, "paired"))?);
    }
    Ok(0)
}

fn unpair(user: i64) -> Result<i32> {
    if store::update(|state| state.unpair(user))? {
        println!("unpaired Telegram user {user}");
        Ok(0)
    } else {
        Err(Error::Config(format!("Telegram user {user} is not paired")))
    }
}

fn status() -> Result<i32> {
    println!(
        "telegram token: {}",
        if token()?.is_some() {
            "stored"
        } else {
            "not set; run `summonr gateway setup telegram`"
        }
    );
    let state = store::load()?;
    let users: Vec<String> = state.users.iter().map(i64::to_string).collect();
    println!(
        "paired users: {}",
        if users.is_empty() {
            "none; message your bot on Telegram to get a pairing code".to_string()
        } else {
            users.join(", ")
        }
    );
    let now = crate::cron::now();
    let waiting = state
        .pending
        .iter()
        .filter(|pending| pending.expires > now)
        .count();
    println!("pending pairings: {waiting}");
    for chat in &state.chats {
        println!("chat {} -> session {}", chat.chat, chat.session);
    }
    Ok(0)
}
