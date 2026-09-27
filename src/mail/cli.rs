use std::path::Path;

use super::*;
use crate::error::{Error, Result};

const USAGE: &str = "usage:\n  summonr mail                  who is on this machine\n  summonr mail agents           same roster\n  summonr mail send <name|id|all> <text>\n  summonr mail inbox <id> [--clear]\n  summonr mail board [get <key>|set <key> <text>]\n\nAny agent CLI can use these: mail is files under ~/.summonr/mail, no server.";

pub fn run(args: &[String], cwd: &Path) -> Result<i32> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    println!("{}", answer(&dir()?, cwd, &words)?);
    Ok(0)
}

fn answer(dir: &Path, cwd: &Path, words: &[&str]) -> Result<String> {
    match words {
        [] | ["agents"] => Ok(roster(&live(dir), "")),
        ["send", to, text @ ..] if !text.is_empty() => send(dir, to, &text.join(" ")),
        ["inbox", id] => read_inbox(dir, id).map(|mail| inbox_text(&mail)),
        ["inbox", id, "--clear"] => take_inbox(dir, id).map(|mail| inbox_text(&mail)),
        ["board"] | ["board", "list"] => read_board(dir, cwd).map(|board| board_text(&board)),
        ["board", "get", key] => read_board(dir, cwd).map(|board| board_entry(&board, key)),
        ["board", "set", key, text @ ..] if !text.is_empty() => {
            post_board(dir, cwd, "cli", key, &text.join(" ")).map(|()| format!("posted {key}"))
        }
        _ => Err(Error::Config(USAGE.to_string())),
    }
}

fn send(dir: &Path, to: &str, text: &str) -> Result<String> {
    let count = deliver_in(dir, "cli", &[to.to_string()], text)?;
    Ok(format!("sent to {count} inbox(es)"))
}
