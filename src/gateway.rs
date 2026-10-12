use std::future::Future;

use crate::error::{Error, Result};

mod child;
pub mod cli;
pub(crate) mod store;
pub(crate) mod telegram;
mod turn;
pub(crate) mod webhook;

use store::State;

pub(crate) const SECRET: &str = "telegram";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Pair,
    Drop,
    Run(String),
    New,
    Status,
    Stop,
    Help,
}

const COMMANDS: [(&str, Action); 5] = [
    ("/new", Action::New),
    ("/status", Action::Status),
    ("/stop", Action::Stop),
    ("/start", Action::Help),
    ("/help", Action::Help),
];

pub(crate) const MENU: [(&str, &str); 4] = [
    ("new", "start a fresh session"),
    ("status", "show the session and the queue"),
    ("stop", "end the running turn"),
    ("help", "how to use this bot"),
];

pub(crate) fn route(state: &State, user: i64, _chat: i64, text: &str, now: i64) -> Action {
    if !state.paired(user) {
        return if state.waiting(user, now) {
            Action::Drop
        } else {
            Action::Pair
        };
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Action::Drop;
    }
    let word = trimmed.split_whitespace().next().unwrap_or_default();
    let command = word.split('@').next().unwrap_or_default();
    COMMANDS
        .iter()
        .find(|(name, _)| *name == command)
        .map_or_else(
            || Action::Run(text.to_string()),
            |(_, action)| action.clone(),
        )
}

pub(crate) fn addressed(text: &str, bot: Option<&str>) -> bool {
    let word = text.split_whitespace().next().unwrap_or_default();
    match word
        .strip_prefix('/')
        .and_then(|command| command.split_once('@'))
    {
        None => true,
        Some((_, name)) => bot.is_some_and(|bot| bot.eq_ignore_ascii_case(name)),
    }
}

pub(crate) fn token() -> Result<Option<String>> {
    Ok(crate::secret::get(SECRET)?
        .map(|(token, _)| token)
        .filter(|token| !token.trim().is_empty()))
}

pub(crate) fn notify(text: &str) -> Result<usize> {
    let Some(token) = token()? else {
        return Ok(0);
    };
    let chats = store::load()?.paired_chats();
    if chats.is_empty() {
        return Ok(0);
    }
    let bot = telegram::Bot::new(&token)?;
    block(async {
        let mut sent = 0;
        for chat in chats {
            if bot.send(chat, text).await.is_ok() {
                sent += 1;
            }
        }
        sent
    })
}

pub(crate) fn block<F: Future>(work: F) -> Result<F::Output> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => Ok(crate::tools::blocking(|| handle.block_on(work))),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map(|runtime| runtime.block_on(work))
            .map_err(|error| Error::Config(format!("cannot start the runtime: {error}"))),
    }
}

#[cfg(test)]
#[path = "gateway_tests.rs"]
mod tests;
