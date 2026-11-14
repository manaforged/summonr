use super::runner::close;
use super::store::{self, Job, Outcome, Run};
use crate::durable::store as durable;
use summonr_core::error::Result;

const STALE: &str =
    "the run stopped without a result: its process was killed or the machine restarted";

pub(super) fn stale(job: &Job, run: &Run, now: i64) -> bool {
    if run.outcome != Outcome::Running {
        return false;
    }
    match run.job.as_deref() {
        Some(id) => crate::jobs::load(&job.dir, id)
            .map_or(true, |record| crate::jobs::finished(&job.dir, &record)),
        None => run.started.saturating_add(budget(job)) <= now,
    }
}

fn budget(job: &Job) -> i64 {
    let secs = job
        .max_secs
        .unwrap_or(crate::limits::limits().cron_budget().as_secs());
    i64::try_from(secs).unwrap_or(i64::MAX)
}

pub(super) fn settle_stale(jobs: &mut [Job], now: i64) {
    for job in jobs.iter_mut() {
        let found: Vec<(i64, i64)> = job
            .runs
            .iter()
            .filter(|run| stale(job, run, now))
            .map(|run| (run.slot, run.started))
            .collect();
        if found.is_empty() {
            job.lock_error = None;
            continue;
        }
        let held = store::run_lock(&job.name).and_then(|lock| Ok(durable::lock(&lock, false)?));
        let held = match held {
            Ok(held) => {
                job.lock_error = None;
                held
            }
            Err(error) => {
                let text = error.to_string();
                if job.lock_error.as_deref() != Some(text.as_str()) {
                    eprintln!(
                        "summonr cron: skipped job {}: cannot take its run lock: {text}",
                        job.name
                    );
                }
                job.lock_error = Some(text);
                continue;
            }
        };
        let Some(_held) = held else {
            continue;
        };
        for (slot, started) in found {
            job.run_at(slot, started).reason = Some(STALE.to_string());
            close(job, slot, started, Outcome::Error);
        }
    }
}

pub(super) fn stop_running(job: &Job) -> Result<bool> {
    let Some(id) = job
        .runs
        .iter()
        .rev()
        .find(|run| run.outcome == Outcome::Running)
        .and_then(|run| run.job.as_deref())
    else {
        return Ok(false);
    };
    let Some(record) = crate::jobs::find(&job.dir, id)? else {
        return Ok(false);
    };
    if crate::jobs::finished(&job.dir, &record) {
        return Ok(false);
    }
    let mut group = crate::tools::command::ProcessGroup::new(record.pid);
    group.disarm();
    Ok(group.terminate())
}
