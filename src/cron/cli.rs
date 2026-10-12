use std::path::Path;

use jiff::Zoned;

use super::args::{self, Show, Spec, USAGE, usage};
use super::report;
use super::runner;
use super::schedule;
use super::store::{self, Job};
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
    eprintln!("{}", view::token_help(name, rotated));
    say(token)
}

fn install() -> Result<i32> {
    super::guard()?;
    let exe = std::env::current_exe()?;
    let every = crate::limits::limits().cron_tick_secs;
    let (path, note) = super::install::install(&exe, every)?;
    say(view::installed(&path, note))
}

fn uninstall() -> Result<i32> {
    super::guard()?;
    say(super::install::uninstall()?.map_or_else(
        || "not installed".to_string(),
        |path| format!("removed {}", path.display()),
    ))
}

fn exec(name: &str, slot: &str, rest: &[&str]) -> Result<i32> {
    let slot = args::number(slot)?;
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
    let text = match spec {
        Spec { manual: true, .. } => Some(schedule::MANUAL.to_string()),
        Spec {
            cron: Some(expr), ..
        } => Some(expr.clone()),
        Spec {
            every: Some(every), ..
        } => Some(schedule::every_text(every)?),
        Spec { at: Some(at), .. } => Some(schedule::at_text(at, &schedule::zone(tz)?)?),
        Spec { .. } => None,
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
    let mut job = Job {
        name: name.clone(),
        dir: cwd.to_path_buf(),
        created: super::now(),
        ..Job::default()
    };
    apply(&mut job, spec)?;
    if job.schedule.is_empty() {
        return Err(usage("add needs --cron, --every, --at or --manual"));
    }
    if spec.task.is_none() {
        return Err(usage("add needs -p PROMPT or --goal GOAL"));
    }
    store::update(|jobs| {
        if jobs.iter().any(|other| other.name == name) {
            return Err(Error::Config(format!("cron job `{name}` already exists")));
        }
        jobs.push(job.clone());
        Ok(())
    })?;
    println!("{}", view::added("added", &job));
    view::warn(&job, spec, true);
    unscheduled()?;
    Ok(0)
}

fn edit(name: &str, spec: &Spec) -> Result<i32> {
    super::guard()?;
    let job = store::with_job(name, |job| {
        apply(job, spec)?;
        Ok(job.clone())
    })?;
    view::warn(&job, spec, false);
    say(view::added("edited", &job))
}

fn remove(name: &str) -> Result<i32> {
    super::guard()?;
    let job = store::update(|jobs| {
        let index = jobs
            .iter()
            .position(|job| job.name == name)
            .ok_or_else(|| store::unknown(name))?;
        Ok(jobs.remove(index))
    })?;
    let stopped = runner::stop_running(&job);
    store::forget_token(name)?;
    store::remove_payloads(&job.dir, name)?;
    store::remove_run_lock(name)?;
    if stopped {
        println!("stopped the running run");
    }
    say(format!("removed {name}"))
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

fn status() -> Result<i32> {
    let installed = super::install::status()?;
    let jobs = store::load()?;
    let now = super::now();
    let next = jobs
        .iter()
        .filter(|job| !job.paused)
        .filter_map(|job| runner::next_fire(job, now))
        .min_by_key(Zoned::timestamp);
    say(view::status(installed.as_deref(), jobs.len(), next))
}
