use std::io::BufRead;
use std::path::{Path, PathBuf};

use super::store::{Job, Outcome, Run};
use crate::Event;
use crate::error::Result;

const NO_ANSWER: &str = "(no answer)";
const STILL_RUNNING: &str = "still running";
const CHECK_FAILED: &str = "check failed";

#[derive(Default)]
pub(super) struct Report {
    text: Option<String>,
    aborted: Option<String>,
    check: Option<(String, bool)>,
    stopped: Option<String>,
}

impl Report {
    pub(super) fn read(log: &Path) -> Self {
        let mut report = Self::default();
        let Ok(file) = std::fs::File::open(log) else {
            return report;
        };
        for line in std::io::BufReader::new(file)
            .lines()
            .map_while(std::result::Result::ok)
        {
            if let Ok(event) = serde_json::from_str::<Event>(&line) {
                report.see(event);
            }
        }
        report
    }

    fn see(&mut self, event: Event) {
        match event {
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

pub(super) fn log_file(job: &Job, run: &Run) -> Option<PathBuf> {
    let id = run.job.as_deref()?;
    let record = crate::jobs::load(&job.dir, id).ok()?;
    Some(crate::jobs::log_path(&job.dir, &record))
}

pub(super) fn render(job: &Job, run: &Run) -> Option<String> {
    let log = log_file(job, run)?;
    let report = Report::read(&log);
    let fallback = match run.outcome {
        Outcome::Running => STILL_RUNNING,
        Outcome::Done
        | Outcome::CheckFailed
        | Outcome::Unfinished
        | Outcome::Error
        | Outcome::Overlap
        | Outcome::Expired => NO_ANSWER,
    };
    let mut lines = vec![report.text.clone().unwrap_or_else(|| fallback.to_string())];
    lines.push(String::new());
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
