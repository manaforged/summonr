use jiff::Zoned;
use jiff::tz::TimeZone;

use super::args::Spec;
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
const NOT_INSTALLED: &str = "installed: no (run `summonr cron install`)";
const NO_ENDPOINT: &str = "none set up; run `summonr login NAME`";
const FIRST_SET_UP: &str = "first set up";

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

pub(super) fn added(verb: &str, job: &Job) -> String {
    format!(
        "{verb} {}: {}, next {}, endpoint {}",
        job.name,
        schedule_label(job),
        next(job),
        endpoint(job)
    )
}

fn endpoint(job: &Job) -> String {
    if let Some(name) = &job.endpoint {
        return name.clone();
    }
    match crate::endpoint::resolve(None, None, crate::endpoint::Need::Key) {
        Ok(found) => {
            let pick = crate::endpoint::chosen_by(None, None)
                .map_or_else(|| FIRST_SET_UP.to_string(), |pick| pick.to_string());
            format!("{} ({pick}; pin one with -e)", found.name)
        }
        Err(_) => NO_ENDPOINT.to_string(),
    }
}

pub(super) fn status(
    installed: Option<&std::path::Path>,
    jobs: usize,
    next: Option<Zoned>,
) -> String {
    let (head, next) = match installed {
        Some(path) => (format!("installed: {}", path.display()), when(next)),
        None => (
            NOT_INSTALLED.to_string(),
            match next {
                Some(at) => format!(
                    "nothing fires until you run `summonr cron install`; would fire at {}",
                    when(Some(at))
                ),
                None => when(None),
            },
        ),
    };
    format!("{head}\njobs: {jobs}\nnext: {next}")
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
            job.runs
                .last()
                .map_or_else(|| "-".to_string(), |run| store::name(&run.outcome)),
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

pub(super) fn show(job: &Job) -> String {
    let tz = zone(job);
    let mut lines = vec![
        format!("name: {}", job.name),
        format!("schedule: {}", schedule_label(job)),
        format!("time zone: {}", tz.iana_name().unwrap_or("system")),
        match &job.task {
            Task::Prompt(text) => format!("prompt: {text}"),
            Task::Goal(text) => format!("goal: {text}"),
        },
        format!("directory: {}", tilde(&job.dir)),
    ];
    lines.push(format!("endpoint: {}", endpoint(job)));
    optional(&mut lines, "model", job.model.as_deref());
    lines.push(format!("budget: {}", budget(job)));
    lines.push(format!(
        "worktree: {}",
        if job.worktree { "yes" } else { "no" }
    ));
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
    let note = note.map(|note| format!("\n{note}")).unwrap_or_default();
    format!("installed {}{note}", path.display())
}

pub(super) fn token_help(name: &str, rotated: bool) -> String {
    let port = crate::limits::limits().gateway_port;
    let lead = if rotated {
        "rotated: the old token no longer works\n"
    } else {
        ""
    };
    format!(
        "{lead}The token is printed once, on stdout. Save it, for example with TOKEN=$(summonr cron token {name}), then fire the job with:\n  curl -X POST -H \"Authorization: Bearer $TOKEN\" http://127.0.0.1:{port}/run/{name}\nafter starting `summonr gateway webhook` (use --port N if you started the listener on another port)."
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

pub(super) fn warn(job: &Job, spec: &Spec, adding: bool) {
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
