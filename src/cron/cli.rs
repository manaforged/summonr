use std::path::Path;

use jiff::Zoned;

use super::args::{self, Show, Spec, USAGE, usage};
use super::report;
use super::runner;
use super::schedule;
use super::store::{self, Job, Notify, Outcome};
use super::view;
use crate::error::{Error, Result};

pub fn run(args: &[String], cwd: &Path) -> Result<i32> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    if args::wants_help(&words) {
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

fn jobs(words: &[&str], cwd: &Path) -> Option<Result<i32>> {
    Some(match words {
        ["add", name, flags @ ..] => args::spec(flags, cwd).and_then(|spec| add(name, &spec, cwd)),
        ["edit", name, flags @ ..] => args::spec(flags, cwd).and_then(|spec| edit(name, &spec)),
        ["list"] => list(false),
        ["list", "--json"] => list(true),
        ["show", name] => show(name),
        ["pause", name] => set_paused(name, true),
        ["resume", name] => set_paused(name, false),
        ["run", name, flags @ ..] => args::show(flags, false).and_then(|show| run_now(name, show)),
        ["remove", name] => remove(name),
        ["log", name, flags @ ..] => args::show(flags, true).and_then(|show| log(name, show)),
        ["token", name] => token(name),
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

fn run_now(name: &str, show: Show) -> Result<i32> {
    super::guard()?;
    let code = runner::exec(name, super::now(), None)?;
    log(
        name,
        Show {
            last: Some(1),
            ..show
        },
    )?;
    Ok(code)
}

fn token(name: &str) -> Result<i32> {
    super::guard()?;
    let rotated = store::token(name).is_some();
    let token = store::rotate_token(name)?;
    eprintln!("{}", view::token_help(name, &token, rotated));
    say(token)
}

fn install() -> Result<i32> {
    super::guard()?;
    let exe = std::env::current_exe()?;
    let every = crate::limits::limits().cron_tick_secs;
    let installed = super::install::install(&exe, every)?;
    say(view::installed(&installed.path, installed.note))
}

fn uninstall() -> Result<i32> {
    super::guard()?;
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

fn schedule_text(spec: &Spec, tz: Option<&str>) -> Result<Option<String>> {
    let given = [&spec.cron, &spec.every, &spec.at]
        .iter()
        .filter(|value| value.is_some())
        .count()
        + usize::from(spec.manual);
    if given > 1 {
        return Err(Error::Config(
            "use one of --cron, --every, --at and --manual".to_string(),
        ));
    }
    let text = if spec.manual {
        Some(schedule::MANUAL.to_string())
    } else if let Some(expr) = &spec.cron {
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
        store::check_task(task)?;
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
        .ok_or_else(|| usage("add needs --cron, --every, --at or --manual"))?;
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
    warn(&job, spec, true);
    unscheduled()?;
    Ok(0)
}

fn edit(name: &str, spec: &Spec) -> Result<i32> {
    super::guard()?;
    let job = store::with_job(name, |job| {
        apply(job, spec)?;
        Ok(job.clone())
    })?;
    warn(&job, spec, false);
    say(view::added("edited", &job))
}

fn remove(name: &str) -> Result<i32> {
    super::guard()?;
    let job = store::update(|jobs| {
        let index = jobs
            .iter()
            .position(|job| job.name == name)
            .ok_or_else(|| Error::Config(format!("no cron job named `{name}`")))?;
        Ok(jobs.remove(index))
    })?;
    let stopped = stop_running(&job);
    store::forget_token(name)?;
    store::remove_payloads(&job.dir, name)?;
    store::remove_run_lock(name)?;
    if stopped {
        println!("stopped the running run");
    }
    say(format!("removed {name}"))
}

fn stop_running(job: &Job) -> bool {
    let Some(id) = job
        .runs
        .iter()
        .rev()
        .find(|run| run.outcome == Outcome::Running)
        .and_then(|run| run.job.as_deref())
    else {
        return false;
    };
    let Ok(record) = crate::jobs::load(&job.dir, id) else {
        return false;
    };
    let running = crate::jobs::status(&job.dir, &record)
        .is_ok_and(|status| status.state == crate::jobs::JobState::Running);
    if !running {
        return false;
    }
    let mut group = crate::tools::command::ProcessGroup::new(record.pid, record.sandbox);
    group.disarm();
    group.terminate()
}

fn set_paused(name: &str, paused: bool) -> Result<i32> {
    super::guard()?;
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
    Ok(0)
}

fn show(name: &str) -> Result<i32> {
    say(view::show(&store::get(name)?))
}

fn log(name: &str, show: Show) -> Result<i32> {
    let job = store::get(name)?;
    let skip = show
        .last
        .map_or(0, |last| job.runs.len().saturating_sub(last));
    let tz = schedule::zone(job.tz.as_deref())?;
    if job.runs.is_empty() {
        return say(view::NO_RUNS.to_string());
    }
    for run in job.runs.iter().skip(skip) {
        println!("{}", view::run_line(run, &tz));
    }
    if let Some(run) = job.runs.iter().rev().find(|run| run.job.is_some()) {
        let text = if show.raw {
            Some(report::raw(&job, run)?)
        } else {
            report::render(&job, run)
        };
        if let Some(text) = text {
            println!("\n{text}");
        }
    }
    Ok(0)
}

fn warn(job: &Job, spec: &Spec, adding: bool) {
    let notes = [
        spec.mail.as_deref().and_then(mail_missing),
        spec.chat.then(chat_missing).flatten(),
        adding
            .then(|| key_missing(job.endpoint.as_deref()))
            .flatten(),
    ];
    for note in notes.into_iter().flatten() {
        eprintln!("warning: {note}");
    }
}

fn mail_missing(session: &str) -> Option<String> {
    let dir = crate::mail::dir().ok()?;
    let found = crate::mail::live(&dir)
        .iter()
        .any(|agent| agent.answers_to(session));
    (!found).then(|| format!("no live session has the name or id `{session}` right now"))
}

fn chat_missing() -> Option<String> {
    if crate::gateway::token().ok().flatten().is_none() {
        return Some(
            "no Telegram token is stored, so --chat sends nothing; run `summonr gateway setup telegram`"
                .to_string(),
        );
    }
    let paired = crate::gateway::store::load()
        .map(|state| state.paired_chats())
        .unwrap_or_default();
    paired.is_empty().then(|| {
        "no Telegram user is paired, so --chat sends nothing; message your bot, then run `summonr gateway pair CODE`"
            .to_string()
    })
}

fn key_missing(endpoint: Option<&str>) -> Option<String> {
    let ready = crate::endpoint::set_up();
    let name = endpoint
        .map(str::to_string)
        .or_else(|| crate::endpoint::default_seat().map(|seat| seat.endpoint));
    match name {
        Some(name) if !ready.contains(&name) => Some(format!(
            "{name} has no usable key; run `summonr login {name}`"
        )),
        None if ready.is_empty() => {
            Some("no endpoint has a key; run `summonr login NAME`".to_string())
        }
        Some(_) | None => None,
    }
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
