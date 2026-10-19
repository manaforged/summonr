use std::iter::Peekable;
use std::vec::IntoIter;

use summonr::{Error, Result};

use super::{Args, exit_help, mcp, value};

pub(super) type Input = Peekable<IntoIter<String>>;
pub(super) type Run = fn(&mut Args, &mut Input) -> Result<()>;
pub(super) type Scope = (&'static str, &'static str, fn(&Args) -> bool, Run);
pub(super) type Switch = (&'static [&'static str], fn(&mut Args));

pub(super) struct Valued {
    pub(super) option: &'static str,
    pub(super) short: Option<&'static str>,
    pub(super) expected: &'static str,
    pub(super) set: fn(&mut Args, String) -> Result<()>,
}

pub(super) const SWITCHES: [Switch; 12] = [
    (&["--json"], |args| args.json = true),
    (&["--trust"], |args| args.trust = true),
    (&["--no-trust"], |args| args.trust = false),
    (&["--strict"], |args| args.strict = true),
    (&["--no-verify"], |args| args.no_verify = true),
    (&["--no-splash"], |args| args.no_splash = true),
    (&["--here"], |args| args.here = true),
    (&["--worktree"], |args| args.worktree = true),
    (&["-c", "--continue"], |args| args.resume = true),
    (&["--fork"], |args| args.fork = true),
    (&["--gui"], |args| args.gui = true),
    (&["--inline"], |args| args.inline = true),
];

pub(super) const VALUES: [Valued; 12] = [
    Valued {
        option: "--add-dir",
        short: None,
        expected: "a directory",
        set: |args, dir| {
            args.add_dir.push(dir);
            Ok(())
        },
    },
    Valued {
        option: "--name",
        short: None,
        expected: "a session name",
        set: |args, name| some(&mut args.name, name),
    },
    Valued {
        option: "--steps",
        short: None,
        expected: "a plan path",
        set: |args, path| some(&mut args.steps, path),
    },
    Valued {
        option: "--goal",
        short: None,
        expected: "an objective",
        set: |args, goal| some(&mut args.goal, goal),
    },
    Valued {
        option: "--session",
        short: None,
        expected: "an id",
        set: |args, id| some(&mut args.session, id),
    },
    Valued {
        option: "--endpoint",
        short: Some("-e"),
        expected: "a name from `summonr endpoints`",
        set: |args, name| some(&mut args.endpoint, name),
    },
    Valued {
        option: "--model",
        short: None,
        expected: "a value",
        set: |args, model| some(&mut args.model, model),
    },
    Valued {
        option: "--workflow",
        short: Some("-w"),
        expected: "a name",
        set: |args, name| some(&mut args.workflow, name),
    },
    Valued {
        option: "--max-tokens",
        short: None,
        expected: "a positive integer",
        set: |args, raw| some(&mut args.max_tokens, count(&raw, "--max-tokens")?),
    },
    Valued {
        option: "--max-secs",
        short: None,
        expected: "a positive integer",
        set: |args, raw| some(&mut args.max_secs, count(&raw, "--max-secs")?),
    },
    Valued {
        option: "--grep",
        short: None,
        expected: "a pattern",
        set: |args, pattern| some(&mut args.grep, pattern),
    },
    Valued {
        option: "--effort",
        short: None,
        expected: "none, minimal, low, medium, high, xhigh, or max",
        set: |args, level| some(&mut args.effort, effort(&level)?),
    },
];

pub(super) const COMMANDS: [(&str, Run); 22] = [
    ("endpoints", |args, _| on(&mut args.list)),
    ("sessions", |args, _| on(&mut args.list_sessions)),
    ("resume", Args::resume),
    ("doctor", |args, _| on(&mut args.doctor)),
    ("classify", |args, _| on(&mut args.classify)),
    ("export", |args, _| on(&mut args.export)),
    ("profile", |args, _| on(&mut args.profile)),
    ("workflows", |args, _| on(&mut args.list_workflows)),
    ("modules", |args, _| on(&mut args.list_modules)),
    ("install", |args, _| on(&mut args.add)),
    ("uninstall", |args, _| on(&mut args.uninstall)),
    ("acp", |args, _| on(&mut args.acp)),
    ("mail", |args, input| {
        some(&mut args.mail, input.by_ref().collect())
    }),
    ("cron", |args, input| {
        some(&mut args.cron, input.by_ref().collect())
    }),
    ("gateway", |args, input| {
        some(&mut args.gateway, input.by_ref().collect())
    }),
    ("tool", |args, input| {
        some(&mut args.tool, input.by_ref().collect())
    }),
    ("mcp", |args, input| some(&mut args.mcp, mcp::parse(input)?)),
    ("login", |args, _| on(&mut args.login)),
    ("logout", |args, _| on(&mut args.logout)),
    ("worktrees", |args, _| on(&mut args.list_worktrees)),
    ("diff", |args, _| on(&mut args.diff)),
    ("help", |_, input| exit_help(input.next().as_deref())),
];

pub(super) const SCOPED: [Scope; 23] = [
    (
        "--stat",
        "diff",
        |args| args.diff,
        |args, _| on(&mut args.stat),
    ),
    (
        "--turn",
        "diff",
        |args| args.diff,
        |args, input| {
            some(
                &mut args.turn,
                value(input, "--turn", "a turn number or `last`")?,
            )
        },
    ),
    (
        "--all",
        "profile",
        |args| args.profile,
        |args, _| on(&mut args.all),
    ),
    ("--last", "profile", |args| args.profile, Args::last),
    (
        "--all",
        "endpoints",
        |args| args.list,
        |args, _| on(&mut args.all),
    ),
    (
        "--check",
        "endpoints",
        |args| args.list,
        |args, _| on(&mut args.live),
    ),
    (
        "--list",
        "install",
        |args| args.add,
        |args, _| on(&mut args.add_list),
    ),
    (
        "--key",
        "login",
        |args| args.login,
        |args, _| on(&mut args.key),
    ),
    (
        "--default",
        "login",
        |args| args.login,
        |args, _| on(&mut args.default),
    ),
    (
        "--url",
        "login",
        |args| args.login,
        |args, input| some(&mut args.url, value(input, "--url", "a base URL")?),
    ),
    (
        "--remove",
        "logout",
        |args| args.logout,
        |args, _| on(&mut args.remove),
    ),
    (
        "--digest",
        "export",
        |args| args.export,
        |args, _| on(&mut args.digest),
    ),
    (
        "--logs",
        "doctor",
        |args| args.doctor,
        |args, _| on(&mut args.logs),
    ),
    (
        "--report",
        "doctor",
        |args| args.doctor,
        |args, _| on(&mut args.report),
    ),
    (
        "--questions",
        "classify",
        |args| args.classify,
        |args, input| {
            some(
                &mut args.questions,
                value(input, "--questions", "JSON or @file")?,
            )
        },
    ),
    (
        "--noul",
        "classify",
        |args| args.classify,
        |args, input| {
            args.noul.push(value(input, "--noul", "a yes/no question")?);
            Ok(())
        },
    ),
    (
        "--review",
        "classify",
        |args| args.classify,
        |args, _| on(&mut args.review),
    ),
    (
        "--set",
        "classify",
        |args| args.classify,
        |args, input| some(&mut args.set, value(input, "--set", "a question set name")?),
    ),
    (
        "--eval",
        "classify",
        |args| args.classify,
        |args, input| {
            some(
                &mut args.eval,
                value(input, "--eval", "a labelled .jsonl path")?,
            )
        },
    ),
    ("--gate", "classify", |args| args.classify, Args::gate),
    (
        "--patch",
        "classify",
        |args| args.classify,
        |args, _| on(&mut args.review_diff),
    ),
    (
        "--diff",
        "classify",
        |args| args.classify,
        |args, _| on(&mut args.review_diff),
    ),
    (
        "prune",
        "worktrees",
        |args| args.list_worktrees,
        |args, _| on(&mut args.prune_worktrees),
    ),
];

fn on(flag: &mut bool) -> Result<()> {
    *flag = true;
    Ok(())
}

fn some<T>(slot: &mut Option<T>, value: T) -> Result<()> {
    *slot = Some(value);
    Ok(())
}

fn count(raw: &str, option: &str) -> Result<u64> {
    raw.parse()
        .ok()
        .filter(|count| *count > 0)
        .ok_or_else(|| Error::Config(format!("{option} requires a positive integer")))
}

fn effort(level: &str) -> Result<summonr::Effort> {
    summonr::Effort::parse(level)
        .ok_or_else(|| Error::Config(format!("--effort {}", summonr::Effort::rejects(level))))
}

impl Args {
    fn resume(&mut self, input: &mut Input) -> Result<()> {
        match input.next_if(|next| !next.starts_with('-')) {
            Some(key) => self.session = Some(key),
            None => self.resume = true,
        }
        Ok(())
    }

    fn last(&mut self, input: &mut Input) -> Result<()> {
        let raw = value(input, "--last", "a session count")?;
        let count = raw
            .parse()
            .map_err(|error| Error::Config(format!("--last wants a count: {error}")))?;
        some(&mut self.last, count)
    }

    fn gate(&mut self, input: &mut Input) -> Result<()> {
        let raw = value(input, "--gate", "a confidence from 0 to 1")?;
        let floor: f64 = raw.parse().map_err(|error| {
            Error::Config(format!("--gate wants a number from 0 to 1: {error}"))
        })?;
        if !(0.0..=1.0).contains(&floor) {
            return Err(Error::Config(
                "--gate wants a number from 0 to 1".to_string(),
            ));
        }
        some(&mut self.gate, floor)
    }
}
