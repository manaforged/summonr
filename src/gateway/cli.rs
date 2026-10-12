use std::path::Path;

use super::telegram::Bot;
use super::turn::{self, Options};
use super::{SECRET, block, store, token};
use crate::error::{Error, Result};

const USAGE: &str = "usage:\n  summonr gateway setup telegram\n  summonr gateway telegram [--endpoint E] [--model M] [--dir D]\n  summonr gateway pair CODE\n  summonr gateway unpair USER_ID\n  summonr gateway status\n  summonr gateway webhook [--port N]\n\nThe Telegram bridge long-polls the Bot API; no port opens.";
const TOKEN_PROMPT: &str = "Telegram bot token: ";
const NO_TOKEN: &str = "no Telegram bot token stored: run `summonr gateway setup telegram`";

pub fn run(
    args: &[String],
    cwd: &Path,
    read_secret: &dyn Fn(&str) -> Result<String>,
) -> Result<i32> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    if asks_help(&words) {
        println!("{USAGE}");
        return Ok(0);
    }
    match words.as_slice() {
        ["setup", "telegram"] => setup(read_secret),
        ["telegram", flags @ ..] => bridge(options(flags, cwd)?),
        ["pair"] => Err(misuse("pair needs the code the bot sent you")),
        ["pair", code] => pair(code),
        ["unpair", user] => unpair(user_id(user)?),
        ["status"] => status(),
        ["webhook", rest @ ..] => webhook(rest),
        [..] => Err(misuse(&format!(
            "unknown gateway command `{}`",
            words.join(" ")
        ))),
    }
}

fn asks_help(words: &[&str]) -> bool {
    words.is_empty()
        || words == ["help"]
        || words.iter().any(|word| matches!(*word, "--help" | "-h"))
}

fn webhook(words: &[&str]) -> Result<i32> {
    match words {
        [] => super::webhook::serve(crate::limits::limits().gateway_port),
        ["--port"] => Err(misuse("--port needs a value")),
        ["--port", port] => super::webhook::serve(
            port.parse()
                .map_err(|error| misuse(&format!("expected a number, got `{port}`: {error}")))?,
        ),
        [..] => Err(misuse(&format!(
            "unknown webhook option `{}`",
            words.join(" ")
        ))),
    }
}

fn misuse(what: &str) -> Error {
    Error::Config(format!("{what}; see `summonr gateway --help`"))
}

fn user_id(text: &str) -> Result<i64> {
    text.parse().ok().ok_or_else(|| {
        Error::Config(
            "USER_ID is a number; `summonr gateway status` lists paired users".to_string(),
        )
    })
}

fn options(words: &[&str], cwd: &Path) -> Result<Options> {
    let mut options = Options {
        dir: cwd.to_path_buf(),
        ..Options::default()
    };
    let mut words = words.iter().copied();
    while let Some(flag) = words.next() {
        let value = words
            .next()
            .ok_or_else(|| misuse(&format!("{flag} needs a value")))?
            .to_string();
        match flag {
            "--endpoint" => options.endpoint = Some(value),
            "--model" => options.model = Some(value),
            "--dir" => options.dir = cwd.join(value),
            unknown => {
                return Err(misuse(&format!("unknown option `{unknown}`")));
            }
        }
    }
    Ok(options)
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
        .ok_or_else(|| Error::Config(format!("no pending pairing with code `{code}`")))?;
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
