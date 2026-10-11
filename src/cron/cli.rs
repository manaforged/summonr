use std::path::{Path, PathBuf};

use jiff::Zoned;

use super::runner;
use super::schedule;
use super::store::{self, Job, Notify, Task};
use crate::error::{Error, Result};

const USAGE: &str = "usage:\n  summonr cron add NAME (--cron EXPR | --every 30m | --at TIME) (-p PROMPT | --goal GOAL)\n      [--dir D] [--endpoint E] [--model M] [--tz ZONE] [--max-secs N] [--max-tokens N]\n      [--worktree] [--notify never|failure|always] [--mail SESSION]\n  summonr cron edit NAME [same flags]\n  summonr cron list [--json]\n  summonr cron show|pause|resume|run|remove NAME\n  summonr cron log NAME [--last N]\n  summonr cron install|uninstall|status\n\nThe OS starts `summonr cron tick` once a minute after `summonr cron install`.";
const TIME_FORMAT: &str = "%Y-%m-%d %H:%M %Z";

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
    notify: Option<Notify>,
    mail: Option<String>,
}

pub fn run(args: &[String], cwd: &Path) -> Result<i32> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["add", name, flags @ ..] => add(name, &spec(flags, cwd)?, cwd),
        ["edit", name, flags @ ..] => edit(name, &spec(flags, cwd)?),
        ["list"] => list(false),
        ["list", "--json"] => list(true),
        ["show", name] => show(name),
        ["pause", name] => set_paused(name, true),
        ["resume", name] => set_paused(name, false),
        ["run", name] => {
            let code = runner::exec(name, super::now())?;
            log(name, Some(1))?;
            Ok(code)
        }
        ["remove", name] => remove(name),
        ["log", name] => log(name, None),
        ["log", name, "--last", count] => log(name, Some(number(count)?)),
        ["install"] => {
            let exe = std::env::current_exe()?;
            let every = crate::limits::limits().cron_tick_secs;
            let path = super::install::install(&exe, every)?;
            say(format!("installed {}", path.display()))
        }
        ["uninstall"] => match super::install::uninstall()? {
            Some(path) => say(format!("removed {}", path.display())),
            None => say("not installed".to_string()),
        },
        ["status"] => status(),
        ["tick"] => runner::tick(super::now()).map(|()| 0),
        ["exec", name, "--slot", slot] => {
            let slot = slot
                .parse()
                .map_err(|error| Error::Config(format!("bad slot `{slot}`: {error}")))?;
            runner::exec(name, slot)
        }
        [..] => Err(Error::Config(USAGE.to_string())),
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

fn spec(words: &[&str], cwd: &Path) -> Result<Spec> {
    let mut spec = Spec::default();
    let mut words = words.iter().copied();
    while let Some(flag) = words.next() {
        if flag == "--worktree" {
            spec.worktree = true;
            continue;
        }
        let value = words
            .next()
            .ok_or_else(|| Error::Config(format!("{flag} needs a value\n{USAGE}")))?
            .to_string();
        match flag {
            "--cron" => spec.cron = Some(value),
            "--every" => spec.every = Some(value),
            "--at" => spec.at = Some(value),
            "-p" | "--prompt" => spec.task = Some(Task::Prompt(value)),
            "--goal" => spec.task = Some(Task::Goal(value)),
            "--dir" => spec.dir = Some(cwd.join(value)),
            "--endpoint" => spec.endpoint = Some(value),
            "--model" => spec.model = Some(value),
            "--tz" => spec.tz = Some(value),
            "--max-secs" => spec.max_secs = Some(number(&value)?),
            "--max-tokens" => spec.max_tokens = Some(number(&value)?),
            "--notify" => spec.notify = Some(notify(&value)?),
            "--mail" => spec.mail = Some(value),
            unknown => {
                return Err(Error::Config(format!(
                    "unknown option `{unknown}`\n{USAGE}"
                )));
            }
        }
    }
    Ok(spec)
}

fn notify(text: &str) -> Result<Notify> {
    serde_json::from_value(serde_json::Value::String(text.to_string())).map_err(|error| {
        Error::Config(format!(
            "bad --notify `{text}`: use never, failure or always ({error})"
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
    job.notify = spec.notify.unwrap_or(job.notify);
    job.mail = spec.mail.clone().or(job.mail.take());
    Ok(())
}

fn add(name: &str, spec: &Spec, cwd: &Path) -> Result<i32> {
    super::guard()?;
    store::check_name(name)?;
    let schedule = schedule_text(spec, spec.tz.as_deref())?
        .ok_or_else(|| Error::Config(format!("add needs --cron, --every or --at\n{USAGE}")))?;
    let task = spec
        .task
        .clone()
        .ok_or_else(|| Error::Config(format!("add needs -p PROMPT or --goal GOAL\n{USAGE}")))?;
    let mut job = Job {
        name: name.to_string(),
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
    say(format!("added {name}: {}", line(&job)))
}

fn edit(name: &str, spec: &Spec) -> Result<i32> {
    super::guard()?;
    let job = store::with_job(name, |job| {
        apply(job, spec)?;
        Ok(job.clone())
    })?;
    say(format!("edited {name}: {}", line(&job)))
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

fn when(zoned: Option<Zoned>) -> String {
    zoned.map_or_else(
        || "-".to_string(),
        |at| at.strftime(TIME_FORMAT).to_string(),
    )
}

fn line(job: &Job) -> String {
    let last = job
        .runs
        .last()
        .map_or_else(|| "-".to_string(), |run| store::name(&run.outcome));
    let next = if job.paused {
        "paused".to_string()
    } else {
        when(runner::next_fire(job, super::now()))
    };
    format!(
        "{}  [{}]  next {next}  last {last}",
        job.schedule,
        job.dir.display()
    )
}

fn list(json: bool) -> Result<i32> {
    for job in store::load()? {
        if json {
            let next = runner::next_fire(&job, super::now()).map(|at| at.timestamp().to_string());
            println!(
                "{}",
                serde_json::json!({
                    "name": job.name,
                    "schedule": job.schedule,
                    "next": next,
                    "last": job.runs.last().map(|run| run.outcome),
                    "paused": job.paused,
                })
            );
        } else {
            println!("{}  {}", job.name, line(&job));
        }
    }
    Ok(0)
}

fn show(name: &str) -> Result<i32> {
    let job = store::get(name)?;
    let text = toml::to_string(&job).map_err(|error| Error::Config(error.to_string()))?;
    say(format!(
        "{text}\nnext: {}",
        when(runner::next_fire(&job, super::now()))
    ))
}

fn log(name: &str, last: Option<usize>) -> Result<i32> {
    let job = store::get(name)?;
    let skip = last.map_or(0, |last| job.runs.len().saturating_sub(last));
    let tz = schedule::zone(job.tz.as_deref())?;
    for run in job.runs.iter().skip(skip) {
        println!(
            "{}  {}  {}",
            when(runner::zoned(run.slot, &tz)),
            store::name(&run.outcome),
            run.job.as_deref().unwrap_or("-")
        );
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
    println!("next: {}", when(next));
    Ok(0)
}
