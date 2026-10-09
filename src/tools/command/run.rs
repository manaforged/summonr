use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::jobs;
use crate::tools::sandbox::Sandbox;
use crate::tools::spill::full_output_note;
use crate::tools::{ToolOutput, ToolRegistry};

use super::Approval;
use super::digest::digest;
use super::proc::{
    CommandChild, ProcessGroup, StartedCommand, job_text, spawn_command, stop_command, wait_shell,
};
use super::rewrite::{plan_write_note, readonly_verb, sandbox_grant, sandbox_note};

const LOG_READ_BYTES: usize = 512 * 1024;
const COMMAND_LOG_PREFIX: &str = "command-";

#[derive(Clone, Copy)]
pub(super) enum Wait {
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
            Self::Kill(seconds) | Self::Detach(seconds) => seconds,
        }
    }
}

struct Parked {
    id: String,
    exit_path: PathBuf,
}

struct JobFiles {
    rec_path: PathBuf,
    exit_path: PathBuf,
}

struct CommandFiles {
    id: String,
    log_path: PathBuf,
    job: Option<JobFiles>,
}

impl ToolRegistry {
    pub(super) fn start_command(
        &self,
        command: &str,
        background: bool,
        read_only: bool,
        approval: Option<Approval>,
        cwd: PathBuf,
    ) -> Result<StartedCommand> {
        let files = self.command_files(background)?;
        let script = match &files.job {
            Some(job) => crate::shell::wrap(command, &job.exit_path, &cwd),
            None => command.to_string(),
        };
        let sandbox = self.command_sandbox(approval)?;
        let child = spawn_command(
            &self.env,
            sandbox.as_ref(),
            &script,
            &files.log_path,
            background,
            read_only,
            &cwd,
        )?;
        let pid = child
            .id()
            .ok_or_else(|| Error::Tool("command started without a pid".to_string()))?;
        if let Some(job) = &files.job {
            jobs::write(&job.rec_path, &self.job_record(&files.id, pid, command)?)?;
        }
        Ok(StartedCommand {
            id: files.id,
            pid,
            log_path: files.log_path,
            child,
        })
    }

    fn command_files(&self, background: bool) -> Result<CommandFiles> {
        if background {
            let (id, rec_path, log_path, exit_path) = jobs::prepare(&self.workspace)?;
            return Ok(CommandFiles {
                id,
                log_path,
                job: Some(JobFiles {
                    rec_path,
                    exit_path,
                }),
            });
        }
        Ok(CommandFiles {
            id: String::new(),
            log_path: self.spill_file(COMMAND_LOG_PREFIX, &[])?,
            job: None,
        })
    }

    fn command_sandbox(&self, approval: Option<Approval>) -> Result<Option<Sandbox>> {
        let one_shot = self.sandbox.is_some()
            && self
                .network_once
                .swap(false, std::sync::atomic::Ordering::SeqCst);
        if matches!(approval, Some(Approval::Uncontained)) {
            return Ok(None);
        }
        if one_shot {
            return self
                .sandbox
                .as_ref()
                .map(Sandbox::allow_network)
                .transpose();
        }
        Ok(self.sandbox.clone())
    }

    pub(super) async fn wait_command(
        &self,
        mut started: StartedCommand,
        command: &str,
        wait: Wait,
        read_only: bool,
        contained: bool,
    ) -> Result<ToolOutput> {
        let mut group = ProcessGroup::new(started.pid, contained);
        let waited = tokio::time::timeout(
            crate::limits::seconds(wait.seconds()),
            wait_shell(&mut started.child),
        )
        .await;
        match waited {
            Ok(Ok(status)) => {
                group.disarm();
                let code = status.code().unwrap_or(-1);
                self.finished(started, command, code, read_only)
            }
            Ok(Err(error)) => Err(Error::Tool(format!("command failed: {error}"))),
            Err(_) => {
                self.timed_out(started, command, wait, group, read_only)
                    .await
            }
        }
    }

    pub(super) fn can_detach(&self, read_only: bool) -> bool {
        !read_only && self.background && self.session_id().is_some()
    }

    fn session_id(&self) -> Option<String> {
        self.session.lock().ok().and_then(|session| session.clone())
    }

    fn job_record(&self, id: &str, pid: u32, command: &str) -> Result<jobs::Record> {
        let mut record = jobs::stamp(id, pid, command, &self.workspace, true, self.session_id())?;
        record.sandbox = self.sandbox.is_some();
        Ok(record)
    }

    fn finished(
        &self,
        started: StartedCommand,
        command: &str,
        code: i32,
        read_only: bool,
    ) -> Result<ToolOutput> {
        let StartedCommand { log_path, .. } = started;
        let raw = scrubbed_preview(&log_path)?;
        let grant = (code != 0)
            .then(|| sandbox_grant(self, command, &raw))
            .flatten();
        let shown = digest(command, &raw);
        let output = format!(
            "exit {code}{}\n{}{}{}",
            full_output_note(
                &log_path,
                shown.cut,
                crate::diff::line_count(&log_path).ok().flatten()
            ),
            shown.text,
            plan_write_note(read_only, &raw),
            sandbox_note(grant.as_ref(), self, &raw)
        );
        keep_log(&log_path, shown.cut);
        Ok(ToolOutput {
            text: output.trim_end().to_string(),
            success: code == 0,
            mutated: !read_only && code == 0 && !readonly_verb(command),
            grant,
            images: Vec::new(),
            patch: None,
        })
    }

    async fn timed_out(
        &self,
        started: StartedCommand,
        command: &str,
        wait: Wait,
        group: ProcessGroup,
        read_only: bool,
    ) -> Result<ToolOutput> {
        let seconds = wait.seconds();
        let mut detail = String::new();
        if matches!(wait, Wait::Detach(_)) {
            match self.register(command, &started) {
                Ok(job) => return self.detach(started, command, seconds, (job, group)),
                Err(error) => detail = format!(" ({error}; could not continue it as a job)"),
            }
        }
        let headline = format!("command timed out after {seconds}s{detail}");
        self.killed(started, command, headline, group, read_only)
            .await
    }

    async fn killed(
        &self,
        mut started: StartedCommand,
        command: &str,
        headline: String,
        mut group: ProcessGroup,
        read_only: bool,
    ) -> Result<ToolOutput> {
        stop_command(&mut started.child, started.pid)
            .map_err(|error| Error::Tool(format!("command cleanup failed: {error}")))?;
        started
            .child
            .wait()
            .await
            .map_err(|error| Error::Tool(format!("command cleanup failed: {error}")))?;
        group.disarm();
        let StartedCommand { log_path, .. } = started;
        let raw = scrubbed_preview(&log_path)?;
        let shown = digest(command, &raw);
        let mutated = !read_only && !readonly_verb(command);
        let warning = if read_only {
            "\nworkspace writes were restricted by the command sandbox"
        } else if mutated {
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
            text: format!("{headline}{warning}{note}\n{}", shown.text)
                .trim_end()
                .to_string(),
            success: false,
            mutated,
            grant: None,
            images: Vec::new(),
            patch: None,
        })
    }

    fn register(&self, command: &str, started: &StartedCommand) -> Result<Parked> {
        let (id, rec_path, _log, exit_path) = jobs::prepare(&self.workspace)?;
        let mut record = self.job_record(&id, started.pid, command)?;
        record.log = Some(started.log_path.display().to_string());
        jobs::write(&rec_path, &record)?;
        Ok(Parked { id, exit_path })
    }

    fn detach(
        &self,
        started: StartedCommand,
        command: &str,
        seconds: u64,
        parked: (Parked, ProcessGroup),
    ) -> Result<ToolOutput> {
        let (job, group) = parked;
        let StartedCommand {
            pid,
            log_path,
            child,
            ..
        } = started;
        let raw =
            crate::askpass::scrub(&jobs::preview(&log_path, LOG_READ_BYTES).unwrap_or_default());
        let shown = digest(command, &raw);
        let text = format!(
            "command still running after {seconds}s; it continues as a background job\n{}\n{}",
            job_text(&job.id, pid, &log_path),
            shown.text
        );
        adopt(child, group, job.exit_path, log_path);
        Ok(ToolOutput {
            text: text.trim_end().to_string(),
            success: true,
            mutated: !readonly_verb(command),
            grant: None,
            images: Vec::new(),
            patch: None,
        })
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

fn adopt(mut child: CommandChild, mut group: ProcessGroup, exit_path: PathBuf, log_path: PathBuf) {
    drop(tokio::spawn(async move {
        let code = wait_shell(&mut child)
            .await
            .map_or(Some(-1), |status| status.code());
        group.disarm();
        if !exit_path.is_file() {
            drop(match code {
                Some(code) => jobs::write_exit(&exit_path, code),
                None => jobs::write_killed(&exit_path),
            });
        }
        crate::askpass::scrub_file(&log_path);
    }));
}
