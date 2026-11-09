use std::io::BufRead;
use std::path::Path;

use super::store::{self, Job, Notify, Outcome, Run};
use crate::Event;
use summonr_core::error::Result;

const NO_ANSWER: &str = "(no answer)";
const STILL_RUNNING: &str = "still running";
const CHECK_FAILED: &str = "check failed";

#[derive(Default)]
pub(crate) struct Report {
    pub(crate) session: Option<String>,
    pub(crate) text: Option<String>,
    pub(crate) aborted: Option<String>,
    check: Option<(String, bool)>,
    stopped: Option<String>,
}

impl Report {
    pub(super) fn load(log: &Path) -> Self {
        let mut report = Self::default();
        let Ok(file) = std::fs::File::open(log) else {
            return report;
        };
        for line in std::io::BufReader::new(file)
            .lines()
            .map_while(std::result::Result::ok)
        {
            report.read(&line);
        }
        report
    }

    pub(crate) fn read(&mut self, line: &str) {
        if let Ok(event) = serde_json::from_str::<Event>(line) {
            self.see(event);
        }
    }

    fn see(&mut self, event: Event) {
        match event {
            Event::Session { id } => self.session = Some(id),
            Event::Assistant { text } if !text.trim().is_empty() => self.text = Some(text),
            Event::Aborted { reason, .. } => self.aborted = Some(reason),
            Event::Verify { command, passed } => self.check = Some((command, passed)),
            Event::Receipt { receipt } => {
                let check = receipt
                    .checks
                    .iter()
                    .find(|check| !check.passed)
                    .or_else(|| receipt.checks.last());
                if let Some(check) = check {
                    self.check = Some((check.command.clone(), check.passed));
                }
            }
            Event::Work { progress } if progress.terminal() => {
                if let Some(reason) = progress.reason {
                    self.stopped = Some(reason);
                }
            }
            other => drop(other),
        }
    }

    pub(super) fn verdict(&self) -> Option<String> {
        if let Some(reason) = &self.aborted {
            return Some(format!("stopped: {reason}"));
        }
        if let Some((command, passed)) = &self.check {
            let word = if *passed { "passed" } else { "failed" };
            return Some(format!("check {word}: {command}"));
        }
        self.stopped
            .as_ref()
            .map(|reason| format!("stopped: {reason}"))
    }

    pub(super) fn reason(&self, outcome: Outcome) -> Option<String> {
        self.aborted.clone().or_else(|| match outcome {
            Outcome::CheckFailed => Some(CHECK_FAILED.to_string()),
            Outcome::Running
            | Outcome::Done
            | Outcome::Unfinished
            | Outcome::Error
            | Outcome::Overlap
            | Outcome::Expired => self.stopped.clone(),
        })
    }
}

pub(super) fn render(job: &Job, run: &Run) -> Option<String> {
    let record = crate::jobs::load(&job.dir, run.job.as_deref()?).ok()?;
    let log = crate::jobs::log_path(&job.dir, &record);
    let report = Report::load(&log);
    let fallback = match run.outcome {
        Outcome::Running => STILL_RUNNING,
        Outcome::Done
        | Outcome::CheckFailed
        | Outcome::Unfinished
        | Outcome::Error
        | Outcome::Overlap
        | Outcome::Expired => NO_ANSWER,
    };
    let mut lines = vec![
        report.text.clone().unwrap_or_else(|| fallback.to_string()),
        String::new(),
    ];
    if let Some(verdict) = report.verdict() {
        lines.push(verdict);
    }
    lines.push(format!("log: {}", log.display()));
    Some(lines.join("\n"))
}

pub(super) fn raw(job: &Job, run: &Run) -> Result<String> {
    let Some(id) = run.job.as_deref() else {
        return Ok(String::new());
    };
    Ok(match crate::jobs::load(&job.dir, id) {
        Ok(record) => crate::jobs::status(&job.dir, &record)?.text,
        Err(error) => error.to_string(),
    })
}

pub(super) fn wanted(level: Notify, outcome: Outcome) -> bool {
    match level {
        Notify::Never => false,
        Notify::Failure => outcome.failed(),
        Notify::Always => true,
    }
}

pub(super) fn notify(job: &Job, outcome: Outcome, reason: Option<&str>, paused: bool) {
    let mut text = format!("cron job {}: {}", job.name, store::name(&outcome));
    if let Some(reason) = reason {
        text.push_str(&format!(" ({reason})"));
    }
    if paused {
        text.push_str(" (paused after repeated failures)");
    }
    if outcome.failed() {
        text.push_str(&format!("; see `summonr cron log {}`", job.name));
    }
    desktop(&text);
    if let Some(session) = &job.mail {
        drop(crate::mail::deliver(
            "cron",
            std::slice::from_ref(session),
            &text,
        ));
    }
    if job.chat {
        drop(crate::gateway::notify(&text));
    }
}

#[cfg(target_os = "macos")]
fn desktop(text: &str) {
    let quoted = text.replace('\\', "\\\\").replace('"', "\\\"");
    quiet(crate::process::Spec::new(
        "/usr/bin/osascript",
        [
            "-e",
            &format!("display notification \"{quoted}\" with title \"summonr\""),
        ],
    ));
}

#[cfg(target_os = "linux")]
fn desktop(text: &str) {
    if let Some(program) = crate::askpass::on_path("notify-send") {
        quiet(crate::process::Spec::new(program, ["summonr", text]));
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn desktop(_: &str) {}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn quiet(spec: crate::process::Spec) {
    if let Ok(mut child) = crate::process::spawn_blocking(crate::process::Spec {
        stdin: crate::process::Io::Null,
        stdout: crate::process::Io::Null,
        stderr: crate::process::Io::Null,
        ..spec
    }) {
        drop(child.wait());
    }
}
