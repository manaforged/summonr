use std::path::Path;

use super::*;
use summonr_core::error::{Error, Result};

/// The `summonr mail --help` text.
pub const USAGE: &str = "usage:\n  summonr mail                  who is on this machine\n  summonr mail agents [--json]  same roster; --json prints each session's state, tool, detail, parent and since\n  summonr mail send [--json] [--reply-to ID] <name|id|all> <text>\n  summonr mail inbox <id> [--clear]\n  summonr mail board [get <key>|set <key> <text>]\n\nAny agent CLI can use these: mail is files under ~/.summonr/mail, no server.";

/// One `summonr mail` command.
#[derive(Clone, Debug)]
pub enum Command {
    /// `mail agents`: who is on this machine.
    Agents,
    /// `mail send TO TEXT`.
    Send {
        /// A session name, id, or `all`.
        to: String,
        /// The message.
        text: String,
        /// The id of the message this answers.
        reply_to: Option<String>,
    },
    /// `mail inbox ID [--clear]`.
    Inbox {
        /// The inbox id.
        id: String,
        /// Empty the inbox after printing it.
        clear: bool,
    },
    /// `mail board`: every entry on this workspace's board.
    Board,
    /// `mail board get KEY`.
    BoardGet(String),
    /// `mail board set KEY TEXT`.
    BoardSet {
        /// The entry key.
        key: String,
        /// The entry text.
        text: String,
    },
}

/// Runs one `summonr mail` command; `json` prints the roster or the send receipt as JSON.
pub fn run(command: &Command, json: bool, cwd: &Path) -> Result<i32> {
    println!("{}", answer(&dir()?, cwd, command, json)?);
    Ok(0)
}

fn answer(dir: &Path, cwd: &Path, command: &Command, json: bool) -> Result<String> {
    match command {
        Command::Agents if json => {
            serde_json::to_string(&live(dir)).map_err(|error| Error::Config(error.to_string()))
        }
        Command::Agents => Ok(roster(&live(dir), "")),
        Command::Send { to, text, reply_to } => send(dir, to, text, reply_to.as_deref(), json),
        Command::Inbox { id, clear: false } => read_inbox(dir, id).map(|mail| inbox_text(&mail)),
        Command::Inbox { id, clear: true } => take_inbox(dir, id).map(|mail| inbox_text(&mail)),
        Command::Board => read_board(dir, cwd).map(|board| board_text(&board)),
        Command::BoardGet(key) => read_board(dir, cwd).map(|board| board_entry(&board, key)),
        Command::BoardSet { key, text } => {
            post_board(dir, cwd, "cli", key, text).map(|()| format!("posted {key}"))
        }
    }
}

fn send(dir: &Path, to: &str, text: &str, reply_to: Option<&str>, json: bool) -> Result<String> {
    let from = std::env::var(crate::session::SESSION_ENV).unwrap_or_else(|_| "cli".to_string());
    let (delivered, id) = deliver_in(dir, &from, &[to.to_string()], text, reply_to)?;
    if json {
        serde_json::to_string(&serde_json::json!({"id": id, "delivered": delivered}))
            .map_err(Error::from)
    } else {
        Ok(format!("sent message {id} to {delivered} inbox(es)"))
    }
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
