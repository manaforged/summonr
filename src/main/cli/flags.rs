use std::iter::Peekable;
use std::vec::IntoIter;

use summonr_agent::table::{self, Row, Table, on, set};
use summonr_agent::{Error, Result};

use super::commands::subcommands;
use super::help::{Manual, exit_manual, exit_version};
use super::{Args, Verb, exit_help, misplaced};

pub(super) type Input = Peekable<IntoIter<String>>;
pub(super) type Run = fn(&mut Args, &mut Input) -> Result<()>;
type Flag = Row<Args>;

pub(super) const FLAGS: Table<Args> = Table {
    rows: &[
        Flag::value(
            &["-e", "--endpoint"],
            "a name from `summonr endpoint list`",
            |args, name| set(&mut args.endpoint, name),
        )
        .help("NAME", "endpoint from `summonr endpoint list`"),
        Flag::value(&["--model"], "a value", |args, model| {
            set(&mut args.model, model)
        })
        .help("NAME", "override the endpoint model"),
        Flag::value(
            &["--effort"],
            "none, minimal, low, medium, high, xhigh, or max",
            |args, level| set(&mut args.effort, effort(&level)?),
        )
        .help("LEVEL", "none, minimal, low, medium, high, xhigh, or max"),
        Flag::value(&["-w", "--workflow"], "a name", |args, name| {
            set(&mut args.workflow, name)
        })
        .help("NAME", "pick each role's endpoint from [workflows.NAME]"),
        Flag::value(&["--skills"], "a name", |args, name| set(&mut args.skills, name))
            .help("NAME", "offer only the skills in [skills.sets.NAME], or none"),
        Flag::switch(&["-p", "--print"], Args::print).help("", "run one prompt and exit"),
        Flag::switch(&["--json"], |args| on(&mut args.json)).help(
            "",
            "print JSON (with -p, session export, session diff, worktree list, endpoint usage, mcp, classify, cron list)",
        ),
        Flag::switch(&["-c", "--continue"], |args| on(&mut args.resume))
            .help("", "resume the last session here"),
        Flag::value(&["--session"], "an id", |args, id| {
            set(&mut args.session, id)
        })
        .help("ID", "resume a session by id, prefix, or name"),
        Flag::switch(&["--fork"], |args| on(&mut args.fork))
            .help("", "copy the resumed session before continuing"),
        Flag::value(&["--name"], "a session name", |args, name| {
            set(&mut args.name, name)
        })
        .help("TEXT", "name this session"),
        Flag::value(&["--steps"], "a plan path", |args, path| {
            set(&mut args.steps, path)
        })
        .help(
            "PLAN.md",
            "run each step under `## Steps` and gate on its check",
        ),
        Flag::value(&["--goal"], "an objective", |args, goal| {
            set(&mut args.goal, goal)
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
        .help("", "ignore SYSTEM.md, AGENTS.md, CLAUDE.md files and rules"),
        Flag::switch(&["--trust"], |args| on(&mut args.trust))
            .help("", "on by default; the last of --trust and --no-trust wins"),
        Flag::switch(&["--here"], |args| on(&mut args.here))
            .help("", "work in this checkout (default; not with --worktree)"),
        Flag::switch(&["--no-verify"], |args| on(&mut args.no_verify))
            .help("", "skip the .summonr/check.toml gate"),
        Flag::switch(&["--worktree"], |args| on(&mut args.worktree))
            .help("", "isolate this session in a git worktree"),
        Flag::value(&["--max-tokens"], "a positive integer", |args, raw| {
            set(&mut args.max_tokens, count(&raw, "--max-tokens")?)
        })
        .help("N", "session token budget; a breach stops the run"),
        Flag::value(&["--max-secs"], "a positive integer", |args, raw| {
            set(&mut args.max_secs, count(&raw, "--max-secs")?)
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
            set(&mut args.grep, pattern)
        }),
        Flag::rest(&["--"], |args, rest| args.prompt.extend(rest)),
        Flag::switch(&["--stat"], |args| on(&mut args.stat))
            .scope("session diff", |args| matches!(args.verb, Some(Verb::Diff))),
        Flag::value(&["--turn"], "a turn number or `last`", |args, turn| {
            set(&mut args.turn, turn)
        })
        .scope("session diff", |args| matches!(args.verb, Some(Verb::Diff))),
        Flag::value(&["--child"], "a child id", |args, child| {
            set(&mut args.child, child)
        })
        .scope("session diff", |args| matches!(args.verb, Some(Verb::Diff))),
        Flag::switch(&["--all"], |args| on(&mut args.all)).scope("session profile", |args| {
            matches!(args.verb, Some(Verb::Profile))
        }),
        Flag::value(&["--last"], "a session count", Args::last).scope("session profile", |args| {
            matches!(args.verb, Some(Verb::Profile))
        }),
        Flag::switch(&["--all"], |args| on(&mut args.all)).scope("endpoint list", |args| {
            matches!(args.verb, Some(Verb::EndpointList))
        }),
        Flag::switch(&["--check"], |args| on(&mut args.live)).scope("endpoint list", |args| {
            matches!(args.verb, Some(Verb::EndpointList))
        }),
        Flag::switch(&["--list"], |args| on(&mut args.add_list)).scope("plugin list", |args| {
            matches!(args.verb, Some(Verb::PluginInstall))
        }),
        Flag::switch(&["--key"], |args| on(&mut args.key))
            .scope("login", |args| matches!(args.verb, Some(Verb::Login))),
        Flag::switch(&["--default"], |args| on(&mut args.default))
            .scope("login", |args| matches!(args.verb, Some(Verb::Login))),
        Flag::value(&["--url"], "a base URL", |args, url| {
            set(&mut args.url, url)
        })
        .scope("endpoint add", |args| {
            matches!(args.verb, Some(Verb::Login))
        }),
        Flag::switch(&["--remove"], |args| on(&mut args.remove))
            .scope("logout", |args| matches!(args.verb, Some(Verb::Logout))),
        Flag::switch(&["--digest"], |args| on(&mut args.digest)).scope("session export", |args| {
            matches!(args.verb, Some(Verb::Export))
        }),
        Flag::switch(&["--logs"], |args| on(&mut args.logs))
            .scope("doctor", |args| matches!(args.verb, Some(Verb::Doctor))),
        Flag::switch(&["--report"], |args| on(&mut args.report))
            .scope("doctor", |args| matches!(args.verb, Some(Verb::Doctor))),
        Flag::value(&["--questions"], "JSON or @file", |args, questions| {
            set(&mut args.questions, questions)
        })
        .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::value(&["--noul"], "a yes/no question", |args, question| {
            args.noul.push(question);
            Ok(())
        })
        .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::switch(&["--review"], |args| on(&mut args.review))
            .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::value(&["--set"], "a question set name", |args, name| {
            set(&mut args.set, name)
        })
        .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::value(&["--eval"], "a labelled .jsonl path", |args, path| {
            set(&mut args.eval, path)
        })
        .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::value(&["--gate"], "a confidence from 0 to 1", Args::gate)
            .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::switch(&["--patch"], |args| on(&mut args.review_diff))
            .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::switch(&["--diff"], |args| on(&mut args.review_diff))
            .scope("classify", |args| matches!(args.verb, Some(Verb::Classify))),
        Flag::switch(&["prune"], |args| on(&mut args.prune_worktrees))
            .scope("worktree list", |args| {
                matches!(args.verb, Some(Verb::Worktrees))
            }),
    ],
    missing: table::requires,
    unknown: table::unknown_option,
    error: Error::Setup,
    dash_values: false,
    pairs: false,
};

pub(super) const ALIASES: [(&str, &str); 11] = [
    ("sessions", "session list"),
    ("resume", "session resume"),
    ("export", "session export"),
    ("profile", "session profile"),
    ("diff", "session diff"),
    ("endpoints", "endpoint list"),
    ("worktrees", "worktree list"),
    ("modules", "module list"),
    ("workflows", "workflow list"),
    ("install", "plugin install"),
    ("uninstall", "plugin remove"),
];

pub(super) type Check = (fn(&Args) -> bool, &'static str);

pub(super) const CONFLICTS: [Check; 4] = [
    (
        |args| args.grep.is_some() && !matches!(args.verb, Some(Verb::SessionList)),
        "--grep works with `summonr session list`; use `summonr session search TEXT`",
    ),
    (
        |args| args.here && args.worktree,
        "use --here or --worktree, not both",
    ),
    (
        |args| args.goal.is_some() && args.steps.is_some(),
        "use --goal or --steps, not both",
    ),
    (
        |args| {
            args.gui
                && (args.verb.is_some()
                    || args.print
                    || args.json
                    || args.inline
                    || args.goal.is_some()
                    || args.steps.is_some())
        },
        "--gui opens the desktop application; omit terminal, print, goal, and subcommand options",
    ),
];

pub(super) fn verb(args: &mut Args, verb: Verb) -> Result<()> {
    args.verb = Some(verb);
    Ok(())
}

pub(super) fn misuse(verb: &str, problem: &str) -> Error {
    let noun = verb.split(' ').next().unwrap_or_default();
    if Manual::of(noun).is_some() {
        table::misuse(noun, problem)
    } else {
        Error::Setup(problem.to_string())
    }
}

fn count(raw: &str, option: &str) -> Result<u64> {
    raw.parse()
        .ok()
        .filter(|count| *count > 0)
        .ok_or_else(|| Error::Setup(format!("{option} requires a positive integer")))
}

fn effort(level: &str) -> Result<summonr_agent::Effort> {
    summonr_agent::Effort::parse(level).ok_or_else(|| {
        Error::Setup(format!(
            "--effort {}",
            summonr_agent::Effort::rejects(level)
        ))
    })
}

impl Args {
    pub(super) fn resume(&mut self, input: &mut Input) -> Result<()> {
        match input.next_if(|next| !next.starts_with('-')) {
            Some(key) => self.session = Some(key),
            None => self.resume = true,
        }
        Ok(())
    }

    pub(super) fn noun(&mut self, noun: &str, input: &mut Input) -> Result<()> {
        let manual = Manual::of(noun);
        if let Some(manual) = manual {
            manual.answer(input);
        }
        while let Some(flag) = input.next_if(|word| word.starts_with('-') && word != "--") {
            let row = FLAGS.find(&flag, self).ok_or_else(|| misplaced(&flag))?;
            FLAGS.apply(row, &flag, self, input)?;
        }
        let verbs = subcommands().filter_map(|row| row.name.strip_prefix(noun)?.strip_prefix(' '));
        let listed = verbs.clone().filter(|verb| Manual::lists(noun, verb));
        let word = input
            .next_if(|word| !word.starts_with('-'))
            .or_else(|| manual.and_then(|manual| manual.bare).map(str::to_string));
        if let Some(manual) = manual
            && word.as_deref() == Some("help")
        {
            exit_manual(manual);
        }
        match word
            .as_deref()
            .and_then(|word| verbs.clone().find(|verb| *verb == word))
        {
            Some(verb) => self.subcommand(&format!("{noun} {verb}"), input),
            None => Err(misuse(
                noun,
                &format!(
                    "`summonr {noun}` takes one of: {}",
                    listed.collect::<Vec<_>>().join(", ")
                ),
            )),
        }
    }

    pub(super) fn subcommand(&mut self, full: &str, input: &mut Input) -> Result<()> {
        let Some(row) = subcommands().find(|row| row.name == full) else {
            return Err(Error::Setup(format!("unknown command `{full}`")));
        };
        self.arity = row.most.map(|most| (row.name, most));
        (row.run)(self, input)?;
        match full.split_once(' ') {
            Some((noun, verb)) => {
                Manual::of(noun).map_or(Ok(()), |manual| manual.seal(verb, &mut self.json, input))
            }
            None => Ok(()),
        }
    }

    pub(super) fn word(&mut self, input: &mut Input, what: &str) -> Result<String> {
        let verb = self.arity.map_or("", |(verb, _)| verb);
        let word = input
            .next_if(|word| !word.starts_with('-'))
            .ok_or_else(|| misuse(verb, &format!("`summonr {verb}` needs {what}")))?;
        self.taken += 1;
        Ok(word)
    }

    pub(super) fn search(&mut self, input: &mut Input) -> Result<()> {
        let pattern = input
            .next()
            .ok_or_else(|| Error::Setup("summonr session search TEXT".to_string()))?;
        set(&mut self.grep, pattern)?;
        verb(self, Verb::SessionList)
    }

    fn last(&mut self, raw: String) -> Result<()> {
        let count = raw
            .parse()
            .map_err(|error| Error::Setup(format!("--last wants a count: {error}")))?;
        set(&mut self.last, count)
    }

    fn gate(&mut self, raw: String) -> Result<()> {
        let floor: f64 = raw
            .parse()
            .map_err(|error| Error::Setup(format!("--gate wants a number from 0 to 1: {error}")))?;
        if !(0.0..=1.0).contains(&floor) {
            return Err(Error::Setup(
                "--gate wants a number from 0 to 1".to_string(),
            ));
        }
        set(&mut self.gate, floor)
    }
}
