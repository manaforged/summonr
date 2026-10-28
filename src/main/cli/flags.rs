use std::iter::Peekable;
use std::vec::IntoIter;

use summonr::table::{Row, Table};
use summonr::{Error, Result};

use super::help::exit_version;
use super::{Args, Verb, exit_help, mcp};

pub(super) type Input = Peekable<IntoIter<String>>;
pub(super) type Run = fn(&mut Args, &mut Input) -> Result<()>;
type Flag = Row<Args>;

pub(super) const FLAGS: Table<Args> = Table {
    rows: &[
        Flag::value(
            &["-e", "--endpoint"],
            "a name from `summonr endpoints`",
            |args, name| some(&mut args.endpoint, name),
        )
        .help("NAME", "endpoint: {endpoints}"),
        Flag::value(&["--model"], "a value", |args, model| {
            some(&mut args.model, model)
        })
        .help("NAME", "override the endpoint model"),
        Flag::value(
            &["--effort"],
            "none, minimal, low, medium, high, xhigh, or max",
            |args, level| some(&mut args.effort, effort(&level)?),
        )
        .help("LEVEL", "none, minimal, low, medium, high, xhigh, or max"),
        Flag::value(&["-w", "--workflow"], "a name", |args, name| {
            some(&mut args.workflow, name)
        })
        .help("NAME", "pick each role's endpoint from [workflows.NAME]"),
        Flag::switch(&["-p", "--print"], Args::print).help("", "run one prompt and exit"),
        Flag::switch(&["--json"], |args| on(&mut args.json)).help(
            "",
            "print JSON (with -p, diff, worktrees, mcp, classify, cron list)",
        ),
        Flag::switch(&["-c", "--continue"], |args| on(&mut args.resume))
            .help("", "resume the last session here"),
        Flag::value(&["--session"], "an id", |args, id| {
            some(&mut args.session, id)
        })
        .help("ID", "resume a session by id, prefix, or name"),
        Flag::switch(&["--fork"], |args| on(&mut args.fork))
            .help("", "copy the resumed session before continuing"),
        Flag::value(&["--name"], "a session name", |args, name| {
            some(&mut args.name, name)
        })
        .help("TEXT", "name this session"),
        Flag::value(&["--steps"], "a plan path", |args, path| {
            some(&mut args.steps, path)
        })
        .help(
            "PLAN.md",
            "run each step under `## Steps` and gate on its check",
        ),
        Flag::value(&["--goal"], "an objective", |args, goal| {
            some(&mut args.goal, goal)
        })
        .help("TEXT", "work toward TEXT across turns"),
        Flag::value(&["--add-dir"], "a directory", |args, dir| {
            args.add_dir.push(dir);
            Ok(())
        })
        .help("DIR", "extra writable root under --strict (repeatable)"),
        Flag::switch(&["--strict"], |args| on(&mut args.strict))
            .help("", "allow only commands listed in policy.toml"),
        Flag::switch(&["--no-trust"], |args| {
            args.trust = false;
            Ok(())
        })
        .help("", "ignore workspace SYSTEM.md, AGENTS.md, CLAUDE.md"),
        Flag::switch(&["--trust"], |args| on(&mut args.trust))
            .help("", "on by default; the last of --trust and --no-trust wins"),
        Flag::switch(&["--here"], |args| on(&mut args.here))
            .help("", "work in this checkout (default; not with --worktree)"),
        Flag::switch(&["--no-verify"], |args| on(&mut args.no_verify))
            .help("", "skip the .summonr/check.toml gate"),
        Flag::switch(&["--worktree"], |args| on(&mut args.worktree))
            .help("", "isolate this session in a git worktree"),
        Flag::value(&["--max-tokens"], "a positive integer", |args, raw| {
            some(&mut args.max_tokens, count(&raw, "--max-tokens")?)
        })
        .help("N", "session token budget; a breach stops the run"),
        Flag::value(&["--max-secs"], "a positive integer", |args, raw| {
            some(&mut args.max_secs, count(&raw, "--max-secs")?)
        })
        .help("N", "session wall-clock budget; a breach stops the run"),
        Flag::switch(&["--no-splash"], |args| on(&mut args.no_splash))
            .help("", "skip the startup banner"),
        Flag::switch(&["--inline"], |args| on(&mut args.inline))
            .help("", "scrollback transcript, composer pinned at the bottom"),
        Flag::switch(&["--gui"], |args| on(&mut args.gui))
            .help("", "open the desktop window (a --features gui build)"),
        Flag::switch(&["-h", "--help"], |args| exit_help(args.verb())).help("", "show this help"),
        Flag::switch(&["-V", "--version"], |_| exit_version()).help("", "show the version"),
        Flag::value(&["--grep"], "a pattern", |args, pattern| {
            some(&mut args.grep, pattern)
        }),
        Flag::rest(&["--"], |args, rest| args.prompt.extend(rest)),
        Flag::switch(&["--stat"], |args| on(&mut args.stat))
            .scope("diff", |args| matches!(args.verb, Some(Verb::Diff))),
        Flag::value(&["--turn"], "a turn number or `last`", |args, turn| {
            some(&mut args.turn, turn)
        })
        .scope("diff", |args| matches!(args.verb, Some(Verb::Diff))),
        Flag::switch(&["--all"], |args| on(&mut args.all))
            .scope("profile", |args| matches!(args.verb, Some(Verb::Profile))),
        Flag::value(&["--last"], "a session count", Args::last)
            .scope("profile", |args| matches!(args.verb, Some(Verb::Profile))),
        Flag::switch(&["--all"], |args| on(&mut args.all)).scope("endpoints", |args| {
            matches!(args.verb, Some(Verb::Endpoints))
        }),
        Flag::switch(&["--check"], |args| on(&mut args.live)).scope("endpoints", |args| {
            matches!(args.verb, Some(Verb::Endpoints))
        }),
        Flag::switch(&["--list"], |args| on(&mut args.add_list))
            .scope("install", |args| matches!(args.verb, Some(Verb::Install))),
        Flag::switch(&["--key"], |args| on(&mut args.key))
            .scope("login", |args| matches!(args.verb, Some(Verb::Login))),
        Flag::switch(&["--default"], |args| on(&mut args.default))
            .scope("login", |args| matches!(args.verb, Some(Verb::Login))),
        Flag::value(&["--url"], "a base URL", |args, url| {
            some(&mut args.url, url)
        })
        .scope("login", |args| matches!(args.verb, Some(Verb::Login))),
        Flag::switch(&["--remove"], |args| on(&mut args.remove))
            .scope("logout", |args| matches!(args.verb, Some(Verb::Logout))),
        Flag::switch(&["--digest"], |args| on(&mut args.digest))
            .scope("export", |args| matches!(args.verb, Some(Verb::Export))),
        Flag::switch(&["--logs"], |args| on(&mut args.logs))
            .scope("doctor", |args| matches!(args.verb, Some(Verb::Doctor))),
        Flag::switch(&["--report"], |args| on(&mut args.report))
            .scope("doctor", |args| matches!(args.verb, Some(Verb::Doctor))),
        Flag::value(&["--questions"], "JSON or @file", |args, questions| {
            some(&mut args.questions, questions)
        })
        .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::value(&["--noul"], "a yes/no question", |args, question| {
            args.noul.push(question);
            Ok(())
        })
        .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::switch(&["--review"], |args| on(&mut args.review))
            .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::value(&["--set"], "a question set name", |args, set| {
            some(&mut args.set, set)
        })
        .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::value(&["--eval"], "a labelled .jsonl path", |args, path| {
            some(&mut args.eval, path)
        })
        .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::value(&["--gate"], "a confidence from 0 to 1", Args::gate)
            .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::switch(&["--patch"], |args| on(&mut args.review_diff))
            .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::switch(&["--diff"], |args| on(&mut args.review_diff))
            .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::switch(&["prune"], |args| on(&mut args.prune_worktrees)).scope("worktrees", |args| {
            matches!(args.verb, Some(Verb::Worktrees))
        }),
    ],
    missing: |flag, expected| format!("{flag} requires {expected}"),
    unknown: |flag| format!("unknown option `{flag}`"),
    error: Error::Config,
    dash_values: false,
    pairs: false,
};

pub(super) const COMMANDS: [(&str, Option<usize>, Run); 23] = [
    ("endpoints", Some(0), |args, _| verb(args, Verb::Endpoints)),
    ("sessions", Some(0), |args, _| verb(args, Verb::Sessions)),
    ("resume", None, Args::resume),
    ("doctor", Some(0), |args, _| verb(args, Verb::Doctor)),
    ("classify", None, |args, _| verb(args, Verb::Classify)),
    ("export", Some(1), |args, _| verb(args, Verb::Export)),
    ("profile", Some(1), |args, _| verb(args, Verb::Profile)),
    ("workflows", Some(0), |args, _| verb(args, Verb::Workflows)),
    ("modules", Some(0), |args, _| verb(args, Verb::Modules)),
    ("install", Some(1), |args, _| verb(args, Verb::Install)),
    ("uninstall", Some(1), |args, _| verb(args, Verb::Uninstall)),
    ("update", Some(0), |args, _| verb(args, Verb::Update)),
    ("acp", Some(0), |args, _| verb(args, Verb::Acp)),
    ("mail", None, |args, input| {
        verb(args, Verb::Mail(input.by_ref().collect()))
    }),
    ("cron", None, |args, input| {
        verb(args, Verb::Cron(input.by_ref().collect()))
    }),
    ("gateway", None, |args, input| {
        verb(args, Verb::Gateway(input.by_ref().collect()))
    }),
    ("tool", None, |args, input| {
        verb(args, Verb::Tool(input.by_ref().collect()))
    }),
    ("mcp", None, |args, input| {
        verb(args, Verb::Mcp(mcp::parse(input)?))
    }),
    ("login", None, |args, _| verb(args, Verb::Login)),
    ("logout", Some(0), |args, _| verb(args, Verb::Logout)),
    ("worktrees", Some(0), |args, _| verb(args, Verb::Worktrees)),
    ("diff", Some(1), |args, _| verb(args, Verb::Diff)),
    ("help", None, |_, input| exit_help(input.next().as_deref())),
];

pub(super) type Check = (fn(&Args) -> bool, &'static str);

pub(super) const CONFLICTS: [Check; 3] = [
    (
        |args| args.grep.is_some() && !matches!(args.verb, Some(Verb::Sessions)),
        "--grep works with `summonr sessions`",
    ),
    (
        |args| args.here && args.worktree,
        "use --here or --worktree, not both",
    ),
    (
        |args| args.goal.is_some() && args.steps.is_some(),
        "use --goal or --steps, not both",
    ),
];

fn on(flag: &mut bool) -> Result<()> {
    *flag = true;
    Ok(())
}

fn verb(args: &mut Args, verb: Verb) -> Result<()> {
    args.verb = Some(verb);
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

    fn last(&mut self, raw: String) -> Result<()> {
        let count = raw
            .parse()
            .map_err(|error| Error::Config(format!("--last wants a count: {error}")))?;
        some(&mut self.last, count)
    }

    fn gate(&mut self, raw: String) -> Result<()> {
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
