use std::path::Path;

use super::*;
use crate::table::{Row, Table};
use summonr_core::error::{Error, Result};

const USAGE: &str = "usage:\n  summonr mail                  who is on this machine\n  summonr mail agents [--json]  same roster; --json prints each session's state, tool, detail, parent and since\n  summonr mail send [--json] [--reply-to ID] <name|id|all> <text>\n  summonr mail inbox <id> [--clear]\n  summonr mail board [get <key>|set <key> <text>|post <text>]\n\nAny agent CLI can use these: mail is files under ~/.summonr/mail, no server.";

#[derive(Default)]
struct Args {
    json: bool,
    reply_to: Option<String>,
}

const SEND: Table<Args> = Table {
    rows: &[
        Row::switch(&["--json"], |args| {
            args.json = true;
            Ok(())
        }),
        Row::value(&["--reply-to"], "a message id", |args, id| {
            args.reply_to = Some(id);
            Ok(())
        }),
    ],
    missing: |flag, expected| format!("{flag} requires {expected}"),
    unknown: |flag| format!("unknown mail send option {flag}"),
    error: Error::Config,
    dash_values: false,
    pairs: false,
};

pub fn run(args: &[String], cwd: &Path) -> Result<i32> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let cwd = dunce::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    println!("{}", answer(&dir()?, &cwd, &words)?);
    Ok(0)
}

fn answer(dir: &Path, cwd: &Path, words: &[&str]) -> Result<String> {
    match words {
        [] | ["agents"] => Ok(roster(&live(dir), "")),
        ["agents", "--json"] => {
            serde_json::to_string(&live(dir)).map_err(|error| Error::Config(error.to_string()))
        }
        ["send", words @ ..] => send(dir, words),
        ["inbox", id] => read_inbox(dir, id).map(|mail| inbox_text(&mail)),
        ["inbox", id, "--clear"] => take_inbox(dir, id).map(|mail| inbox_text(&mail)),
        ["board"] | ["board", "list"] => Ok(board_text(&read_board(dir, cwd)?, &read_feed(dir, cwd)?)),
        ["board", "get", key] => read_board(dir, cwd).map(|board| board_entry(&board, key)),
        ["board", "set", key, text @ ..] if !text.is_empty() => {
            post_board(dir, cwd, "cli", key, &text.join(" ")).map(|()| format!("posted {key}"))
        }
        ["board", "post", text @ ..] if !text.is_empty() => say(dir, cwd, &text.join(" ")),
        [..] => Err(Error::Config(USAGE.to_string())),
    }
}

fn say(dir: &Path, cwd: &Path, text: &str) -> Result<String> {
    let mail = Mail {
        board: true,
        ..sender_mail(&live(dir), &cli_sender(), text, None)
    };
    let (delivered, id) = say_in(dir, cwd, mail)?;
    Ok(format!("posted message {id} to {delivered} agent(s) on this board"))
}

fn cli_sender() -> String {
    std::env::var(crate::session::SESSION_ENV).unwrap_or_else(|_| "cli".to_string())
}

fn send(dir: &Path, words: &[&str]) -> Result<String> {
    let mut args = Args::default();
    let mut input = words.iter().map(|word| word.to_string()).peekable();
    while input.peek().is_some_and(|word| word.starts_with('-')) {
        let flag = input.next().expect("invariant: peek found a word");
        if flag == "--" {
            break;
        }
        let row = SEND
            .find(&flag, &args)
            .ok_or_else(|| (SEND.error)((SEND.unknown)(&flag)))?;
        SEND.apply(row, &flag, &mut args, &mut input)?;
    }
    let to = input
        .next()
        .ok_or_else(|| Error::Config(USAGE.to_string()))?;
    let text = input.collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err(Error::Config(USAGE.to_string()));
    }
    let from = cli_sender();
    let (delivered, id) = deliver_in(dir, &from, &[to], &text, args.reply_to.as_deref())?;
    if args.json {
        serde_json::to_string(&serde_json::json!({"id": id, "delivered": delivered}))
            .map_err(Error::from)
    } else {
        Ok(format!("sent message {id} to {delivered} inbox(es)"))
    }
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
