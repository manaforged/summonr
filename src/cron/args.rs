use std::path::{Path, PathBuf};

use super::store::{Notify, Task};
use super::view;
use crate::error::{Error, Result};

pub(super) const USAGE: &str = "usage:\n  summonr cron add NAME (--cron EXPR | --every 30m | --at TIME | --manual) (-p PROMPT | --goal GOAL)\n      [--dir D] [--endpoint E] [--model M] [--tz ZONE] [--max-secs N] [--max-tokens N]\n      [--worktree] [--chat] [--notify never|failure|always] [--mail SESSION]\n  summonr cron edit NAME [same flags]\n  summonr cron list [--json]\n  summonr cron show|pause|resume|remove NAME\n  summonr cron run NAME [--raw]\n  summonr cron log NAME [--last N] [--raw]\n  summonr cron token NAME\n  summonr cron install|uninstall|status\n\nTIME is HH:MM, YYYY-MM-DD HH:MM, or RFC 3339. A --manual job runs only through `cron run` or the webhook.\nThe OS starts `summonr cron tick` once a minute after `summonr cron install`.";
const HELP: [&str; 2] = ["--help", "-h"];
const LAST: &str = "--last";
const RAW: &str = "--raw";

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
}

pub(super) fn wants_help(words: &[&str]) -> bool {
    matches!(words, [] | ["help", ..]) || words.iter().any(|word| HELP.contains(word))
}

pub(super) fn usage(message: &str) -> Error {
    Error::Config(format!("{message}{}", view::HELP_HINT))
}

pub(super) fn number<T: std::str::FromStr<Err = std::num::ParseIntError>>(text: &str) -> Result<T> {
    text.parse()
        .map_err(|error| Error::Config(format!("expected a number, got `{text}`: {error}")))
}

pub(super) fn show(words: &[&str], last: bool) -> Result<Show> {
    let mut show = Show::default();
    let mut words = words.iter().copied();
    while let Some(flag) = words.next() {
        match flag {
            RAW => show.raw = true,
            LAST if last => {
                let value = words.next().ok_or_else(|| missing(flag))?;
                show.last = Some(number(value)?);
            }
            unknown => return Err(usage(&format!("unknown option `{unknown}`"))),
        }
    }
    Ok(show)
}

fn missing(flag: &str) -> Error {
    usage(&format!("{flag} needs a value"))
}

type Switch = fn(&mut Spec);
type Valued = fn(&mut Spec, String, &Path) -> Result<()>;

const SWITCHES: [(&str, Switch); 3] = [
    ("--worktree", |spec| spec.worktree = true),
    ("--chat", |spec| spec.chat = true),
    ("--manual", |spec| spec.manual = true),
];

const VALUES: [(&str, Valued); 14] = [
    ("--cron", |spec, value, _| set(&mut spec.cron, value)),
    ("--every", |spec, value, _| set(&mut spec.every, value)),
    ("--at", |spec, value, _| set(&mut spec.at, value)),
    ("-p", |spec, value, _| {
        set(&mut spec.task, Task::Prompt(value))
    }),
    ("--prompt", |spec, value, _| {
        set(&mut spec.task, Task::Prompt(value))
    }),
    ("--goal", |spec, value, _| {
        set(&mut spec.task, Task::Goal(value))
    }),
    ("--dir", |spec, value, cwd| {
        set(&mut spec.dir, cwd.join(value))
    }),
    ("--endpoint", |spec, value, _| {
        set(&mut spec.endpoint, value)
    }),
    ("--model", |spec, value, _| set(&mut spec.model, value)),
    ("--tz", |spec, value, _| set(&mut spec.tz, value)),
    ("--max-secs", |spec, value, _| {
        set(&mut spec.max_secs, number(&value)?)
    }),
    ("--max-tokens", |spec, value, _| {
        set(&mut spec.max_tokens, number(&value)?)
    }),
    ("--notify", |spec, value, _| {
        set(&mut spec.notify, notify(&value)?)
    }),
    ("--mail", |spec, value, _| set(&mut spec.mail, value)),
];

fn set<T>(slot: &mut Option<T>, value: T) -> Result<()> {
    *slot = Some(value);
    Ok(())
}

pub(super) fn spec(words: &[&str], cwd: &Path) -> Result<Spec> {
    let mut spec = Spec::default();
    let mut words = words.iter().copied();
    while let Some(flag) = words.next() {
        // complexity: bounded by the three cron switches
        if let Some((_, switch)) = SWITCHES.iter().find(|(name, _)| *name == flag) {
            switch(&mut spec);
            continue;
        }
        // complexity: bounded by the fourteen cron options
        let (_, apply) = VALUES
            .iter()
            .find(|(name, _)| *name == flag)
            .ok_or_else(|| usage(&format!("unknown option `{flag}`")))?;
        let value = words.next().ok_or_else(|| missing(flag))?;
        apply(&mut spec, value.to_string(), cwd)?;
    }
    Ok(spec)
}

fn notify(text: &str) -> Result<Notify> {
    Notify::parse(text).ok_or_else(|| {
        Error::Config(format!(
            "bad --notify `{text}`: use never, failure or always"
        ))
    })
}
