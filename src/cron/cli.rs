use std::path::{Path, PathBuf};

use jiff::Zoned;

use super::runner;
use super::schedule;
use super::store::{self, Job, Notify, Task};
use super::view;
use crate::error::{Error, Result};

const USAGE: &str = "usage:\n  summonr cron add NAME (--cron EXPR | --every 30m | --at TIME) (-p PROMPT | --goal GOAL)\n      [--dir D] [--endpoint E] [--model M] [--tz ZONE] [--max-secs N] [--max-tokens N]\n      [--worktree] [--chat] [--notify never|failure|always] [--mail SESSION]\n  summonr cron edit NAME [same flags]\n  summonr cron list [--json]\n  summonr cron show|pause|resume|run|remove NAME\n  summonr cron log NAME [--last N]\n  summonr cron token NAME\n  summonr cron install|uninstall|status\n\nThe OS starts `summonr cron tick` once a minute after `summonr cron install`.";

#[derive(Default)]
struct Spec {
    cron: Option<String>,
    every: Option<String>,
    at: Option<String>,
    task: Option<Task>,
    dir: Option<PathBuf>,
    endpoint: Option<String>,
    model: Option<String>,
    tz: Option<String>,
    max_secs: Option<u64>,
    max_tokens: Option<u64>,
    worktree: bool,
    chat: bool,
    notify: Option<Notify>,
    mail: Option<String>,
}

pub fn run(args: &[String], cwd: &Path) -> Result<i32> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    if matches!(words[..], [] | ["help" | "--help" | "-h"]) {
        return say(USAGE.to_string());
    }
    jobs(&words, cwd)
        .or_else(|| system(&words))
        .unwrap_or_else(|| {
            Err(usage(&format!(
                "unknown cron command `{}`",
                words.join(" ")
            )))
        })
}

fn usage(message: &str) -> Error {
    Error::Config(format!("{message}{}", view::HELP_HINT))
}

fn jobs(words: &[&str], cwd: &Path) -> Option<Result<i32>> {
    Some(match words {
        ["add", name, flags @ ..] => spec(flags, cwd).and_then(|spec| add(name, &spec, cwd)),
        ["edit", name, flags @ ..] => spec(flags, cwd).and_then(|spec| edit(name, &spec)),
        ["list"] => list(false),
        ["list", "--json"] => list(true),
        ["show", name] => show(name),
        ["pause", name] => set_paused(name, true),
        ["resume", name] => set_paused(name, false),
        ["run", name] => run_now(name),
        ["remove", name] => remove(name),
        ["log", name] => log(name, None),
        ["log", name, "--last", count] => number(count).and_then(|count| log(name, Some(count))),
        ["token", name] => {
            store::rotate_token(name).and_then(|token| say(view::token(name, &token)))
        }
        [..] => return None,
    })
}

fn system(words: &[&str]) -> Option<Result<i32>> {
    Some(match words {
        ["install"] => install(),
        ["uninstall"] => uninstall(),
        ["status"] => status(),
        ["tick"] => runner::tick(super::now()).map(|()| 0),
        ["exec", name, "--slot", slot, rest @ ..] => exec(name, slot, rest),
        [..] => return None,
    })
}

fn run_now(name: &str) -> Result<i32> {
    let code = runner::exec(name, super::now(), None)?;
    log(name, Some(1))?;
    Ok(code)
}

fn install() -> Result<i32> {
    let exe = std::env::current_exe()?;
    let every = crate::limits::limits().cron_tick_secs;
    let path = super::install::install(&exe, every)?;
    say(format!("installed {}", path.display()))
}

fn uninstall() -> Result<i32> {
    match super::install::uninstall()? {
        Some(path) => say(format!("removed {}", path.display())),
        None => say("not installed".to_string()),
    }
}

fn exec(name: &str, slot: &str, rest: &[&str]) -> Result<i32> {
    let slot = slot
        .parse()
        .map_err(|error| Error::Config(format!("bad slot `{slot}`: {error}")))?;
    match rest {
        [] => runner::exec(name, slot, None),
        ["--payload", file] => runner::exec(name, slot, Some(Path::new(file))),
        [..] => Err(usage("exec takes NAME --slot N [--payload FILE]")),
    }
}

fn say(text: String) -> Result<i32> {
    println!("{text}");
    Ok(0)
}

fn number<T: std::str::FromStr<Err = std::num::ParseIntError>>(text: &str) -> Result<T> {
    text.parse()
        .map_err(|error| Error::Config(format!("expected a number, got `{text}`: {error}")))
}

type Switch = fn(&mut Spec);
type Valued = fn(&mut Spec, String, &Path) -> Result<()>;

const SWITCHES: [(&str, Switch); 2] = [
    ("--worktree", |spec| spec.worktree = true),
    ("--chat", |spec| spec.chat = true),
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

fn spec(words: &[&str], cwd: &Path) -> Result<Spec> {
    let mut spec = Spec::default();
    let mut words = words.iter().copied();
    while let Some(flag) = words.next() {
        // complexity: bounded by the two cron switches
        if let Some((_, switch)) = SWITCHES.iter().find(|(name, _)| *name == flag) {
            switch(&mut spec);
            continue;
        }
        // complexity: bounded by the fourteen cron options
        let (_, apply) = VALUES
            .iter()
            .find(|(name, _)| *name == flag)
            .ok_or_else(|| usage(&format!("unknown option `{flag}`")))?;
        let value = words
            .next()
            .ok_or_else(|| usage(&format!("{flag} needs a value")))?;
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

fn schedule_text(spec: &Spec, tz: Option<&str>) -> Result<Option<String>> {
    let given = [&spec.cron, &spec.every, &spec.at]
        .iter()
        .filter(|value| value.is_some())
        .count();
    if given > 1 {
        return Err(Error::Config(
            "use one of --cron, --every and --at".to_string(),
        ));
    }
    let text = if let Some(expr) = &spec.cron {
        Some(expr.clone())
    } else if let Some(every) = &spec.every {
        Some(schedule::every_text(every)?)
    } else if let Some(at) = &spec.at {
        Some(schedule::at_text(at, &schedule::zone(tz)?)?)
    } else {
        None
    };
    if let Some(text) = &text {
        schedule::parse(text)?;
    }
    Ok(text)
}

fn apply(job: &mut Job, spec: &Spec) -> Result<()> {
    if let Some(tz) = &spec.tz {
        schedule::zone(Some(tz))?;
        job.tz = Some(tz.clone());
    }
    if let Some(text) = schedule_text(spec, job.tz.as_deref())? {
        job.schedule = text;
    }
    if let Some(task) = &spec.task {
        job.task = task.clone();
    }
    if let Some(dir) = &spec.dir {
        job.dir.clone_from(dir);
    }
    job.endpoint = spec.endpoint.clone().or(job.endpoint.take());
    job.model = spec.model.clone().or(job.model.take());
    job.max_secs = spec.max_secs.or(job.max_secs);
    job.max_tokens = spec.max_tokens.or(job.max_tokens);
    job.worktree = job.worktree || spec.worktree;
    job.chat = job.chat || spec.chat;
    job.notify = spec.notify.unwrap_or(job.notify);
    job.mail = spec.mail.clone().or(job.mail.take());
    Ok(())
}

fn add(name: &str, spec: &Spec, cwd: &Path) -> Result<i32> {
    super::guard()?;
    let name = name.to_ascii_lowercase();
    store::check_name(&name)?;
    let schedule = schedule_text(spec, spec.tz.as_deref())?
        .ok_or_else(|| usage("add needs --cron, --every or --at"))?;
    let task = spec
        .task
        .clone()
        .ok_or_else(|| usage("add needs -p PROMPT or --goal GOAL"))?;
    let mut job = Job {
        name: name.clone(),
        schedule,
        tz: None,
        task,
        endpoint: None,
        model: None,
        dir: cwd.to_path_buf(),
        max_secs: None,
        max_tokens: None,
        worktree: false,
        notify: Notify::default(),
        mail: None,
        chat: false,
        paused: false,
        fails: 0,
        created: super::now(),
        runs: Vec::new(),
    };
    apply(&mut job, spec)?;
    store::update(|jobs| {
        if jobs.iter().any(|other| other.name == name) {
            return Err(Error::Config(format!("cron job `{name}` already exists")));
        }
        jobs.push(job.clone());
        Ok(())
    })?;
    println!("{}", view::added("added", &job));
    unscheduled()?;
    Ok(0)
}

fn edit(name: &str, spec: &Spec) -> Result<i32> {
    super::guard()?;
    let job = store::with_job(name, |job| {
        apply(job, spec)?;
        Ok(job.clone())
    })?;
    say(view::added("edited", &job))
}

fn remove(name: &str) -> Result<i32> {
    super::guard()?;
    store::update(|jobs| {
        let before = jobs.len();
        jobs.retain(|job| job.name != name);
        if jobs.len() == before {
            return Err(Error::Config(format!("no cron job named `{name}`")));
        }
        Ok(())
    })?;
    store::forget_token(name)?;
    say(format!("removed {name}"))
}

fn set_paused(name: &str, paused: bool) -> Result<i32> {
    store::with_job(name, |job| {
        job.paused = paused;
        if !paused {
            job.fails = 0;
        }
        Ok(())
    })?;
    say(format!(
        "{} {name}",
        if paused { "paused" } else { "resumed" }
    ))
}

fn unscheduled() -> Result<()> {
    if super::install::status()?.is_none() {
        println!("{}", view::UNSCHEDULED);
    }
    Ok(())
}

fn list(json: bool) -> Result<i32> {
    let jobs = store::load()?;
    if json {
        for job in jobs {
            println!("{}", view::json(&job));
        }
        return Ok(0);
    }
    if jobs.is_empty() {
        println!("{}", view::EMPTY);
    } else {
        let header = std::io::IsTerminal::is_terminal(&std::io::stdout());
        println!("{}", view::table(&jobs, header));
    }
    unscheduled()?;
    Ok(0)
}

fn show(name: &str) -> Result<i32> {
    say(view::show(&store::get(name)?))
}

fn log(name: &str, last: Option<usize>) -> Result<i32> {
    let job = store::get(name)?;
    let skip = last.map_or(0, |last| job.runs.len().saturating_sub(last));
    let tz = schedule::zone(job.tz.as_deref())?;
    if job.runs.is_empty() {
        return say(view::NO_RUNS.to_string());
    }
    for run in job.runs.iter().skip(skip) {
        println!("{}", view::run_line(run, &tz));
    }
    if let Some(id) = job.runs.iter().rev().find_map(|run| run.job.as_deref()) {
        match crate::jobs::load(&job.dir, id) {
            Ok(record) => println!("\n{}", crate::jobs::status(&job.dir, &record)?.text),
            Err(error) => println!("\n{error}"),
        }
    }
    Ok(0)
}

fn status() -> Result<i32> {
    match super::install::status()? {
        Some(path) => println!("installed: {}", path.display()),
        None => println!("installed: no (run `summonr cron install`)"),
    }
    let jobs = store::load()?;
    let now = super::now();
    let next = jobs
        .iter()
        .filter(|job| !job.paused)
        .filter_map(|job| runner::next_fire(job, now))
        .min_by_key(Zoned::timestamp);
    println!("jobs: {}", jobs.len());
    println!("next: {}", view::when(next));
    Ok(0)
}
