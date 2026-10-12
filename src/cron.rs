use std::ffi::OsStr;

use crate::error::{Error, Result};

pub mod cli;
mod install;
pub(crate) mod runner;
pub(crate) mod schedule;
pub(crate) mod store;
mod view;

pub(crate) fn now() -> i64 {
    i64::try_from(crate::mail::now()).unwrap_or(i64::MAX)
}

pub(crate) fn guard() -> Result<()> {
    refuse(std::env::var_os(runner::ENV).as_deref())
}

fn refuse(cron: Option<&OsStr>) -> Result<()> {
    match cron {
        Some(_) => Err(Error::Denied(
            "a cron run cannot change cron jobs".to_string(),
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "cron_tests.rs"]
mod tests;
