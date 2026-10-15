use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::jobs;
use crate::tools::spill::full_output_note;
use crate::tools::{ToolOutput, ToolRegistry};

use super::digest::digest;
use super::proc::{
    ProcessGroup, StartedCommand, background_result, spawn_command, stop_command, wait_shell,
};

const LOG_READ_BYTES: usize = 512 * 1024;
const COMMAND_LOG_PREFIX: &str = "command-";

#[derive(Clone, Copy)]
pub(super) enum Wait {
    Background,
    Kill(u64),
    Detach(u64),
}

impl Wait {
    pub(super) fn chosen(explicit: Option<u64>, detach: bool) -> Self {
        match explicit {
            Some(seconds) => Self::Kill(seconds),
            None if detach => Self::Detach(crate::limits::limits().command_wait_secs),
            None => Self::Kill(crate::limits::limits().command_kill_secs),
        }
    }

    fn seconds(self) -> u64 {
        match self {
            Self::Background => 0,
            Self::Kill(seconds) | Self::Detach(seconds) => seconds,
        }
    }
}

impl ToolRegistry {
    pub(super) fn start_command(
        &self,
        command: &str,
        wait: Wait,
        cwd: PathBuf,
    ) -> Result<StartedCommand> {
        let persist = matches!(wait, Wait::Background | Wait::Detach(_));
        let (id, log_path, record_path) = if persist {
            let (id, record_path, log_path, exit_path) = jobs::prepare(&self.workspace)?;
            let script = crate::shell::wrap(command, &exit_path, &cwd);
            (id, log_path, Some((record_path, script)))
        } else {
            (
                String::new(),
                self.spill_file(COMMAND_LOG_PREFIX, &[])?,
                None,
            )
        };
        let script = record_path
            .as_ref()
            .map_or(command, |(_, script)| script.as_str());
        let child = spawn_command(&self.env, script, &log_path, persist, &cwd)?;
        let pid = child
            .id()
            .ok_or_else(|| Error::Tool("command started without a pid".to_string()))?;
        let group = ProcessGroup::new(pid);
        if let Some((record_path, _)) = record_path.filter(|_| matches!(wait, Wait::Background)) {
            jobs::write(&record_path, &self.job_record(&id, pid, command)?)?;
        }
        Ok(StartedCommand {
            id,
            pid,
            log_path,
            child,
            group,
        })
    }

    pub(super) async fn wait_command(
        &self,
        mut started: StartedCommand,
        command: &str,
        wait: Wait,
    ) -> Result<ToolOutput> {
        let waited = tokio::time::timeout(
            crate::limits::seconds(wait.seconds()),
            wait_shell(&mut started.child),
        )
        .await;
        match waited {
            Ok(Ok(status)) => {
                stop_command(&mut started.child, started.pid)
                    .map_err(|error| Error::Tool(format!("command cleanup failed: {error}")))?;
                started.group.disarm();
                self.discard_job(&started.id);
                let code = status.code().unwrap_or(-1);
                self.finished(started.log_path, command, code)
            }
            Ok(Err(error)) => Err(Error::Tool(format!("command failed: {error}"))),
            Err(_) => self.timed_out(started, command, wait).await,
        }
    }

    pub(super) fn can_detach(&self, read_only: bool) -> bool {
        !read_only && self.background && self.session_id().is_some()
    }

    fn session_id(&self) -> Option<String> {
        self.session.lock().ok().and_then(|session| session.clone())
    }

    fn job_record(&self, id: &str, pid: u32, command: &str) -> Result<jobs::Record> {
        jobs::stamp(id, pid, command, &self.workspace, true, self.session_id())
    }

    fn finished(&self, log_path: PathBuf, command: &str, code: i32) -> Result<ToolOutput> {
        let raw = scrubbed_preview(&log_path)?;
        let shown = digest(command, &raw);
        let output = format!(
            "exit {code}{}\n{}",
            full_output_note(
                &log_path,
                shown.cut,
                crate::diff::line_count(&log_path).ok().flatten()
            ),
            shown.text,
        );
        keep_log(&log_path, shown.cut);
        Ok(ToolOutput {
            text: output.trim_end().to_string(),
            success: code == 0,
            mutated: may_write(command),
            grant: None,
            images: Vec::new(),
            patch: None,
        })
    }

    async fn timed_out(
        &self,
        mut started: StartedCommand,
        command: &str,
        wait: Wait,
    ) -> Result<ToolOutput> {
        let seconds = wait.seconds();
        let mut detail = String::new();
        if matches!(wait, Wait::Detach(_)) {
            match self.register(command, &started) {
                Ok(()) => return self.detach(started, command, seconds),
                Err(error) => detail = format!(" ({error}; could not continue it as a job)"),
            }
        }
        let cleanup =
            |error: std::io::Error| Error::Tool(format!("command cleanup failed: {error}"));
        stop_command(&mut started.child, started.pid).map_err(cleanup)?;
        started.child.wait().await.map_err(cleanup)?;
        started.group.disarm();
        self.discard_job(&started.id);
        let log_path = started.log_path;
        let raw = scrubbed_preview(&log_path)?;
        let shown = digest(command, &raw);
        let mutated = may_write(command);
        let warning = if mutated {
            "\nthe command may have changed workspace files before timeout"
        } else {
            ""
        };
        let note = full_output_note(
            &log_path,
            shown.cut,
            crate::diff::line_count(&log_path).ok().flatten(),
        );
        keep_log(&log_path, shown.cut);
        Ok(ToolOutput {
            text: format!(
                "command timed out after {seconds}s{detail}{warning}{note}\n{}",
                shown.text
            )
            .trim_end()
            .to_string(),
            success: false,
            mutated,
            grant: None,
            images: Vec::new(),
            patch: None,
        })
    }

    fn register(&self, command: &str, started: &StartedCommand) -> Result<()> {
        let record = self.job_record(&started.id, started.pid, command)?;
        jobs::write(
            &jobs::dir(&self.workspace).join(format!("{}.json", started.id)),
            &record,
        )
    }

    fn discard_job(&self, id: &str) {
        if !id.is_empty() {
            drop(std::fs::remove_file(jobs::exit_path(&self.workspace, id)));
        }
    }

    fn detach(&self, started: StartedCommand, command: &str, seconds: u64) -> Result<ToolOutput> {
        let raw = crate::askpass::scrub(&jobs::preview(&started.log_path, LOG_READ_BYTES)?);
        let shown = digest(command, &raw);
        let mut output = background_result(started);
        output.text = format!(
            "command still running after {seconds}s; it continues as a background job\n{}\n{}",
            output.text, shown.text
        )
        .trim_end()
        .to_string();
        output.mutated = may_write(command);
        Ok(output)
    }
}

fn keep_log(log_path: &std::path::Path, keep: bool) {
    if !keep {
        drop(std::fs::remove_file(log_path));
    }
}

fn scrubbed_preview(log_path: &std::path::Path) -> Result<String> {
    let scrubbed = crate::askpass::scrub_file(log_path);
    let preview = jobs::preview(log_path, LOG_READ_BYTES)?;
    Ok(if scrubbed {
        crate::askpass::scrub(&preview)
    } else {
        preview
    })
}

fn may_write(command: &str) -> bool {
    !crate::policy::reads_only(command)
}
