use jiff::Zoned;
use jiff::tz::TimeZone;

use super::runner;
use super::schedule::{self, Schedule};
use super::store::{self, Job, Run, Task};
use crate::config::tilde;

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M %Z";
const SHOWN_RUNS: usize = 5;
const HEADER: [&str; 5] = ["NAME", "SCHEDULE", "NEXT", "LAST", "DIR"];
pub(super) const HELP_HINT: &str = "; see `summonr cron --help`";
pub(super) const EMPTY: &str =
    "no cron jobs; add one with `summonr cron add NAME --every 1d -p PROMPT`";
pub(super) const UNSCHEDULED: &str =
    "not scheduled yet: run `summonr cron install` once so jobs fire";
pub(super) const NO_RUNS: &str = "no runs yet";

pub(super) fn when(zoned: Option<Zoned>) -> String {
    zoned.map_or_else(
        || "-".to_string(),
        |at| at.strftime(TIME_FORMAT).to_string(),
    )
}

fn zone(job: &Job) -> TimeZone {
    schedule::zone(job.tz.as_deref()).unwrap_or_else(|_| TimeZone::system())
}

pub(super) fn schedule_label(job: &Job) -> String {
    match schedule::parse(&job.schedule) {
        Ok(Schedule::At(stamp)) => format!("at {}", when(Some(stamp.to_zoned(zone(job))))),
        Ok(Schedule::Cron(_) | Schedule::Every(_) | Schedule::Manual) | Err(_) => {
            job.schedule.clone()
        }
    }
}

fn next(job: &Job) -> String {
    if job.paused {
        "paused".to_string()
    } else {
        when(runner::next_fire(job, super::now()))
    }
}

fn last(job: &Job) -> String {
    job.runs
        .last()
        .map_or_else(|| "-".to_string(), |run| store::name(&run.outcome))
}

pub(super) fn added(verb: &str, job: &Job) -> String {
    format!(
        "{verb} {}: {}, next {}",
        job.name,
        schedule_label(job),
        next(job)
    )
}

pub(super) fn table(jobs: &[Job], header: bool) -> String {
    let mut rows: Vec<[String; 5]> = Vec::new();
    if header {
        rows.push(HEADER.map(str::to_string));
    }
    rows.extend(jobs.iter().map(|job| {
        [
            job.name.clone(),
            schedule_label(job),
            next(job),
            last(job),
            tilde(&job.dir),
        ]
    }));
    let mut widths = [0usize; 5];
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    rows.iter()
        .map(|row| align(row, &widths))
        .collect::<Vec<_>>()
        .join("\n")
}

fn align(row: &[String; 5], widths: &[usize; 5]) -> String {
    let cells: Vec<String> = row
        .iter()
        .zip(widths)
        .map(|(cell, width)| format!("{cell:<width$}"))
        .collect();
    cells.join("  ").trim_end().to_string()
}

pub(super) fn run_line(run: &Run, tz: &TimeZone) -> String {
    format!(
        "{}  {}  {}",
        when(runner::zoned(run.slot, tz)),
        store::name(&run.outcome),
        run.job.as_deref().unwrap_or("-")
    )
}

fn task(job: &Job) -> String {
    match &job.task {
        Task::Prompt(text) => format!("prompt: {text}"),
        Task::Goal(text) => format!("goal: {text}"),
    }
}

fn budget(job: &Job) -> String {
    let time = job.max_secs.map_or_else(
        || {
            format!(
                "{} (default)",
                crate::text::duration(crate::limits::limits().cron_budget_secs)
            )
        },
        crate::text::duration,
    );
    match job.max_tokens {
        Some(tokens) => format!("{time}, {tokens} tokens"),
        None => time,
    }
}

fn state(job: &Job) -> String {
    let base = if job.paused { "paused" } else { "active" };
    if job.fails > 0 {
        format!("{base}, {} failures in a row", job.fails)
    } else {
        base.to_string()
    }
}

fn optional(lines: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        lines.push(format!("{label}: {value}"));
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

pub(super) fn show(job: &Job) -> String {
    let tz = zone(job);
    let mut lines = vec![
        format!("name: {}", job.name),
        format!("schedule: {}", schedule_label(job)),
        format!("time zone: {}", tz.iana_name().unwrap_or("system")),
        task(job),
        format!("directory: {}", tilde(&job.dir)),
    ];
    optional(&mut lines, "endpoint", job.endpoint.as_deref());
    optional(&mut lines, "model", job.model.as_deref());
    lines.push(format!("budget: {}", budget(job)));
    lines.push(format!("worktree: {}", yes_no(job.worktree)));
    lines.push(format!("notify: {}", store::name(&job.notify)));
    optional(&mut lines, "mail", job.mail.as_deref());
    if job.chat {
        lines.push("chat: yes".to_string());
    }
    lines.push(format!("state: {}", state(job)));
    lines.push(format!(
        "created: {}",
        when(runner::zoned(job.created, &tz))
    ));
    lines.push(format!("next: {}", next(job)));
    lines.push(runs(job, &tz));
    lines.join("\n")
}

fn runs(job: &Job, tz: &TimeZone) -> String {
    if job.runs.is_empty() {
        return format!("runs: {NO_RUNS}");
    }
    let skip = job.runs.len().saturating_sub(SHOWN_RUNS);
    let lines: Vec<String> = job
        .runs
        .iter()
        .skip(skip)
        .map(|run| format!("  {}", run_line(run, tz)))
        .collect();
    format!("runs:\n{}", lines.join("\n"))
}

pub(super) fn installed(path: &std::path::Path, note: Option<&str>) -> String {
    match note {
        Some(note) => format!("installed {}\n{note}", path.display()),
        None => format!("installed {}", path.display()),
    }
}

pub(super) fn token_help(name: &str, token: &str, rotated: bool) -> String {
    let port = crate::limits::limits().gateway_port;
    let lead = if rotated {
        "rotated: the old token no longer works\n"
    } else {
        ""
    };
    format!(
        "{lead}Shown once. Fire the job with:\n  curl -X POST -H 'Authorization: Bearer {token}' http://127.0.0.1:{port}/run/{name}\nafter starting `summonr gateway webhook` (use --port N if you started the listener on another port)."
    )
}

pub(super) fn json(job: &Job) -> serde_json::Value {
    let next = runner::next_fire(job, super::now())
        .filter(|_| !job.paused)
        .map(|at| at.timestamp().to_string());
    serde_json::json!({
        "name": job.name,
        "schedule": job.schedule,
        "next": next,
        "last": job.runs.last().map(|run| run.outcome),
        "paused": job.paused,
        "dir": job.dir,
        "task": job.task,
        "endpoint": job.endpoint,
    })
}
