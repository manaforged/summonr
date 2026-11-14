use std::path::{Path, PathBuf};

use super::store::{Notify, Task};
use crate::command::table::{self, Row, Table, on, set};
use summonr_core::error::{Error, Result};

/// The `summonr cron --help` text.
pub const USAGE: &str = "usage:\n  summonr cron add NAME (--cron EXPR | --every 30m | --at TIME | --manual) (-p PROMPT | --goal GOAL)\n      [--dir D] [-e|--endpoint E] [--model M] [--tz ZONE] [--max-secs N] [--max-tokens N]\n      [--worktree] [--chat] [--notify never|failure|always] [--mail SESSION] [--wake SESSION]\n  summonr cron edit NAME [same flags]\n  summonr cron list [--json]\n  summonr cron show|pause|resume|remove NAME\n  summonr cron run NAME [--raw] [--notify never|failure|always]\n  summonr cron log NAME [--last N] [--raw]\n  summonr cron token NAME\n  summonr cron install|uninstall|status\n\nTIME is HH:MM, YYYY-MM-DD HH:MM, or RFC 3339. A --manual job runs only through `cron run` or the webhook.\nThe OS starts `summonr cron tick` once a minute after `summonr cron install`.\n--wake SESSION delivers -p PROMPT directly to that live session as a wake-up, without starting a model or another session.";

/// The schedule, task and options of a `cron add` or `cron edit`.
#[derive(Clone, Debug, Default)]
pub struct Spec {
    pub(super) cron: Option<String>,
    pub(super) every: Option<String>,
    pub(super) at: Option<String>,
    pub(super) manual: bool,
    pub(super) task: Option<Task>,
    pub(super) dir: Option<PathBuf>,
    pub(super) endpoint: Option<String>,
    pub(super) model: Option<String>,
    pub(super) tz: Option<String>,
    pub(super) max_secs: Option<u64>,
    pub(super) max_tokens: Option<u64>,
    pub(super) worktree: bool,
    pub(super) chat: bool,
    pub(super) notify: Option<Notify>,
    pub(super) mail: Option<String>,
    pub(super) wake: Option<String>,
}

/// The options of a `cron run` or `cron log`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Show {
    pub(super) last: Option<usize>,
    pub(super) raw: bool,
    pub(super) notify: Option<Notify>,
}

pub(super) fn usage(message: &str) -> Error {
    table::misuse("cron", message)
}

fn number<T: std::str::FromStr<Err = std::num::ParseIntError>>(text: &str) -> Result<T> {
    table::number(text).map_err(|message| usage(&message))
}

const fn flags<T>(rows: &'static [Row<T>]) -> Table<T> {
    Table {
        rows,
        missing: table::needs_value,
        unknown: table::unknown_option,
        error: misuse,
        dash_values: true,
        pairs: false,
    }
}

fn misuse(message: String) -> Error {
    usage(&message)
}

type ShowFlag = Row<Show>;

const RUN_ROWS: &[ShowFlag] = &[
    ShowFlag::switch(&["--raw"], raw),
    ShowFlag::value(&["--notify"], "", |show, value| {
        show.notify = Some(notify(&value)?);
        Ok(())
    }),
];

const RUN: Table<Show> = flags(RUN_ROWS);

const LOG_ROWS: &[ShowFlag] = &[
    ShowFlag::switch(&["--raw"], raw),
    ShowFlag::value(&["--last"], "", |show, value| {
        show.last = Some(number(&value)?);
        Ok(())
    }),
];

const LOG: Table<Show> = flags(LOG_ROWS);

fn raw(show: &mut Show) -> Result<()> {
    show.raw = true;
    Ok(())
}

impl Show {
    /// Reads the flags of `cron log` when `log` is set, else of `cron run`.
    pub fn parse(words: impl IntoIterator<Item = String>, log: bool) -> Result<Self> {
        let mut show = Self::default();
        let table = if log { LOG } else { RUN };
        table.walk(&mut show, words)?;
        Ok(show)
    }
}

type SpecFlag = Row<Spec>;

const SPEC_ROWS: &[SpecFlag] = &[
    SpecFlag::switch(&["--worktree"], |spec| on(&mut spec.worktree)),
    SpecFlag::switch(&["--chat"], |spec| on(&mut spec.chat)),
    SpecFlag::switch(&["--manual"], |spec| on(&mut spec.manual)),
    SpecFlag::value(&["--cron"], "", |spec, value| set(&mut spec.cron, value)),
    SpecFlag::value(&["--every"], "", |spec, value| set(&mut spec.every, value)),
    SpecFlag::value(&["--at"], "", |spec, value| set(&mut spec.at, value)),
    SpecFlag::value(&["-p", "--prompt"], "", |spec, value| {
        set(&mut spec.task, Task::Prompt(value))
    }),
    SpecFlag::value(&["--goal"], "", |spec, value| {
        set(&mut spec.task, Task::Goal(value))
    }),
    SpecFlag::value(&["--dir"], "", |spec, value| {
        set(&mut spec.dir, PathBuf::from(value))
    }),
    SpecFlag::value(&["--endpoint", "-e"], "", |spec, value| {
        set(&mut spec.endpoint, value)
    }),
    SpecFlag::value(&["--model"], "", |spec, value| set(&mut spec.model, value)),
    SpecFlag::value(&["--tz"], "", |spec, value| set(&mut spec.tz, value)),
    SpecFlag::value(&["--max-secs"], "", |spec, value| {
        set(&mut spec.max_secs, number(&value)?)
    }),
    SpecFlag::value(&["--max-tokens"], "", |spec, value| {
        set(&mut spec.max_tokens, number(&value)?)
    }),
    SpecFlag::value(&["--notify"], "", |spec, value| {
        set(&mut spec.notify, notify(&value)?)
    }),
    SpecFlag::value(&["--mail"], "", |spec, value| set(&mut spec.mail, value)),
    SpecFlag::value(&["--wake"], "", |spec, value| set(&mut spec.wake, value)),
];

const SPEC: Table<Spec> = flags(SPEC_ROWS);

impl Spec {
    /// Reads the flags of `cron add` and `cron edit`.
    pub fn parse(words: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut spec = Self::default();
        SPEC.walk(&mut spec, words)?;
        Ok(spec)
    }

    pub(super) fn rooted(&self, cwd: &Path) -> Self {
        Self {
            dir: self.dir.as_ref().map(|dir| cwd.join(dir)),
            ..self.clone()
        }
    }
}

fn notify(text: &str) -> Result<Notify> {
    Notify::parse(text).ok_or_else(|| {
        usage(&format!(
            "bad --notify `{text}`: use never, failure or always"
        ))
    })
}
