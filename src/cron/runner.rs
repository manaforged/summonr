use std::fs::File;
use std::process::{Command, Stdio};

use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};

use super::schedule::{self, Schedule};
use super::store::{self, Job, Notify, Outcome, Run, Task};
use crate::durable::store as durable;
use crate::error::Result;

pub(crate) const ENV: &str = "SUMMONR_CRON";
const TICK: &str = "tick";
const RUNS: &str = "runs";
const SOURCE: &str = "cron";
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Due {
    Run(i64),
    Expired(i64),
    No,
}

pub(crate) fn zoned(secs: i64, tz: &TimeZone) -> Option<Zoned> {
    Timestamp::from_second(secs)
        .ok()
        .map(|stamp| stamp.to_zoned(tz.clone()))
}

pub(crate) fn due(job: &Job, last_slot: Option<i64>, now: i64) -> Result<Due> {
    let plan = schedule::parse(&job.schedule)?;
    let tz = schedule::zone(job.tz.as_deref())?;
    let since = last_slot.unwrap_or(job.created);
    let catchup = i64::try_from(crate::limits::limits().cron_catchup_secs).unwrap_or(i64::MAX);
    let floor = now.saturating_sub(catchup);
    if let Some(slot) = latest(&plan, &tz, since.max(floor), now) {
        return Ok(Due::Run(slot));
    }
    Ok(latest(&plan, &tz, since, floor.min(now)).map_or(Due::No, Due::Expired))
}

fn latest(plan: &Schedule, tz: &TimeZone, from: i64, to: i64) -> Option<i64> {
    let mut found = None;
    let mut at = from;
    while let Some(next) = zoned(at, tz).and_then(|after| plan.next_after(after)) {
        let slot = next.timestamp().as_second();
        if slot > to {
            break;
        }
        found = Some(slot);
        at = slot;
    }
    found
}

pub(crate) fn next_fire(job: &Job, now: i64) -> Option<Zoned> {
    let plan = schedule::parse(&job.schedule).ok()?;
    let tz = schedule::zone(job.tz.as_deref()).ok()?;
    plan.next_after(zoned(now.max(job.last_slot().unwrap_or(now)), &tz)?)
}

pub(crate) fn tick(now: i64) -> Result<()> {
    let Some(_guard) = durable::lock(&store::dir()?.join(TICK), false)? else {
        return Ok(());
    };
    let starts = store::update(|jobs| {
        let mut starts = Vec::new();
        for job in jobs.iter_mut().filter(|job| !job.paused) {
            let (slot, outcome) = match due(job, job.last_slot(), now).unwrap_or(Due::No) {
                Due::Run(slot) => (slot, Outcome::Running),
                Due::Expired(slot) => (slot, Outcome::Expired),
                Due::No => continue,
            };
            job.runs.push(Run {
                slot,
                started: now,
                job: None,
                outcome,
            });
            if outcome == Outcome::Running {
                starts.push((job.name.clone(), slot));
            }
        }
        Ok(starts)
    })?;
    let exe = std::env::current_exe()?;
    for (name, slot) in starts {
        let mut command = Command::new(&exe);
        command
            .args(["cron", "exec", &name, "--slot", &slot.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(
            &mut command,
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
        );
        command.spawn()?;
    }
    Ok(())
}

pub(crate) fn exec(name: &str, slot: i64) -> Result<i32> {
    let job = store::get(name)?;
    let started = super::now();
    let Some(_guard) = durable::lock(&store::dir()?.join(RUNS).join(name), false)? else {
        store::settle(name, slot, started, |run| run.outcome = Outcome::Overlap)?;
        eprintln!("cron job `{name}` is still running; this run was skipped");
        return Ok(1);
    };
    let workspace = job.dir.as_path();
    let (id, record_path, log, exit) = crate::jobs::prepare(workspace)?;
    let out = File::create(&log)?;
    let err = out.try_clone()?;
    let mut child = Command::new(std::env::current_exe()?)
        .args(argv(&job))
        .current_dir(workspace)
        .env(ENV, name)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()?;
    let command = format!("summonr cron exec {name}");
    let record = crate::jobs::stamp(&id, child.id(), &command, workspace, false, None)?;
    crate::jobs::write(&record_path, &record)?;
    store::settle(name, slot, started, |run| run.job = Some(id.clone()))?;
    let status = child.wait()?;
    match status.code() {
        Some(code) => durable::write(&exit, format!("{code}\n"))?,
        None => crate::jobs::write_killed(&exit)?,
    }
    let outcome = Outcome::from_code(status.code());
    let paused = finish(name, slot, started, outcome)?;
    if notify_wanted(job.notify, outcome) {
        notify(&job, outcome, paused);
    }
    Ok(status.code().unwrap_or(1))
}

fn finish(name: &str, slot: i64, started: i64, outcome: Outcome) -> Result<bool> {
    store::settle(name, slot, started, |run| run.outcome = outcome)?;
    store::with_job(name, |job| {
        job.fails = if outcome.failed() {
            job.fails.saturating_add(1)
        } else {
            0
        };
        let one_shot = matches!(schedule::parse(&job.schedule), Ok(Schedule::At(_)));
        let streak = job.fails >= crate::limits::limits().cron_fail_pause;
        job.paused = job.paused || one_shot || streak;
        Ok(streak)
    })
}

fn argv(job: &Job) -> Vec<String> {
    let mut args: Vec<String> = job.endpoint.iter().cloned().collect();
    if let Some(model) = &job.model {
        args.extend(["--model".to_string(), model.clone()]);
    }
    let max_secs = job
        .max_secs
        .unwrap_or(crate::limits::limits().cron_budget_secs);
    args.extend([
        "--json".to_string(),
        "--max-secs".to_string(),
        max_secs.to_string(),
    ]);
    if let Some(tokens) = job.max_tokens {
        args.extend(["--max-tokens".to_string(), tokens.to_string()]);
    }
    if job.worktree {
        args.push("--worktree".to_string());
    }
    match &job.task {
        Task::Prompt(prompt) => args.extend(["-p".to_string(), prompt.clone()]),
        Task::Goal(goal) => args.extend(["--goal".to_string(), goal.clone()]),
    }
    args
}

fn notify_wanted(level: Notify, outcome: Outcome) -> bool {
    match level {
        Notify::Never => false,
        Notify::Failure => outcome.failed(),
        Notify::Always => true,
    }
}

fn notify(job: &Job, outcome: Outcome, paused: bool) {
    let mut text = format!("cron job {}: {}", job.name, store::name(&outcome));
    if paused {
        text.push_str(" (paused after repeated failures)");
    }
    desktop(&text);
    if let Some(session) = &job.mail {
        drop(crate::mail::deliver(
            SOURCE,
            std::slice::from_ref(session),
            &text,
        ));
    }
}

#[cfg(target_os = "macos")]
fn desktop(text: &str) {
    let quoted = text.replace('\\', "\\\\").replace('"', "\\\"");
    quiet(Command::new("/usr/bin/osascript").args([
        "-e",
        &format!("display notification \"{quoted}\" with title \"summonr\""),
    ]));
}

#[cfg(target_os = "linux")]
fn desktop(text: &str) {
    if let Some(program) = crate::askpass::on_path("notify-send") {
        quiet(Command::new(program).args(["summonr", text]));
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn desktop(_: &str) {}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn quiet(command: &mut Command) {
    drop(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    );
}
