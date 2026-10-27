use std::path::{Path, PathBuf};

use super::store::{Notify, Task};
use super::view;
use crate::command::table::{self, Row, Table};
use crate::error::{Error, Result};

pub(super) const USAGE: &str = "usage:\n  summonr cron add NAME (--cron EXPR | --every 30m | --at TIME | --manual) (-p PROMPT | --goal GOAL)\n      [--dir D] [-e|--endpoint E] [--model M] [--tz ZONE] [--max-secs N] [--max-tokens N]\n      [--worktree] [--chat] [--notify never|failure|always] [--mail SESSION]\n  summonr cron edit NAME [same flags]\n  summonr cron list [--json]\n  summonr cron show|pause|resume|remove NAME\n  summonr cron run NAME [--raw] [--notify never|failure|always]\n  summonr cron log NAME [--last N] [--raw]\n  summonr cron token NAME\n  summonr cron install|uninstall|status\n\nTIME is HH:MM, YYYY-MM-DD HH:MM, or RFC 3339. A --manual job runs only through `cron run` or the webhook.\nThe OS starts `summonr cron tick` once a minute after `summonr cron install`.";

#[derive(Default)]
pub(super) struct Spec {
    pub cron: Option<String>,
    pub every: Option<String>,
    pub at: Option<String>,
    pub manual: bool,
    pub task: Option<Task>,
    pub dir: Option<PathBuf>,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub tz: Option<String>,
    pub max_secs: Option<u64>,
    pub max_tokens: Option<u64>,
    pub worktree: bool,
    pub chat: bool,
    pub notify: Option<Notify>,
    pub mail: Option<String>,
}

#[derive(Clone, Copy, Default)]
pub(super) struct Show {
    pub last: Option<usize>,
    pub raw: bool,
    pub notify: Option<Notify>,
}

pub(super) fn wants_help(words: &[&str]) -> bool {
    matches!(words, [] | ["help", ..]) || table::asks_help(words)
}

pub(super) fn usage(message: &str) -> Error {
    Error::Setup(format!("{message}{}", view::HELP_HINT))
}

pub(super) fn number<T: std::str::FromStr<Err = std::num::ParseIntError>>(text: &str) -> Result<T> {
    table::number(text).map_err(Error::Config)
}

const fn flags<T>(rows: &'static [Row<T>]) -> Table<T> {
    Table {
        rows,
        missing: needs_value,
        unknown: unknown_option,
        error: misuse,
        dash_values: true,
        pairs: false,
    }
}

fn needs_value(flag: &str, _: &str) -> String {
    format!("{flag} needs a value")
}

fn unknown_option(flag: &str) -> String {
    format!("unknown option `{flag}`")
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

pub(super) fn show(words: &[&str], last: bool) -> Result<Show> {
    let mut show = Show::default();
    let table = if last { LOG } else { RUN };
    table.walk(&mut show, words.iter().map(ToString::to_string))?;
    Ok(show)
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
];

const SPEC: Table<Spec> = flags(SPEC_ROWS);

fn on(flag: &mut bool) -> Result<()> {
    *flag = true;
    Ok(())
}

fn set<T>(slot: &mut Option<T>, value: T) -> Result<()> {
    *slot = Some(value);
    Ok(())
}

pub(super) fn spec(words: &[&str], cwd: &Path) -> Result<Spec> {
    let mut spec = Spec::default();
    SPEC.walk(&mut spec, words.iter().map(ToString::to_string))?;
    spec.dir = spec.dir.map(|dir| cwd.join(dir));
    Ok(spec)
}

fn notify(text: &str) -> Result<Notify> {
    Notify::parse(text).ok_or_else(|| {
        usage(&format!(
            "bad --notify `{text}`: use never, failure or always"
        ))
    })
}
