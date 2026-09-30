use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::jobs;
use crate::tools::sandbox::Sandbox;
use crate::tools::spill::full_output_note;
use crate::tools::{ToolOutput, ToolRegistry};

use super::digest::digest;
use super::proc::{
    CommandChild, ProcessGroup, StartedCommand, job_text, spawn_command, stop_command, wait_shell,
};
use super::rewrite::{plan_write_note, readonly_verb, sandbox_grant, sandbox_note};

const LOG_READ_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy)]
pub(super) enum Wait {
    Kill(u64),
    Detach(u64),
}

impl Wait {
    pub(super) fn chosen(explicit: Option<u64>, detach: bool) -> Self {
        match explicit {
            Some(seconds) => Self::Kill(seconds),
            None if detach => Self::Detach(crate::limits::COMMAND_WAIT_SECS),
            None => Self::Kill(crate::limits::COMMAND_KILL_SECS),
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

struct CommandFiles {
    temp: Option<tempfile::TempDir>,
    id: String,
    rec_path: PathBuf,
    log_path: PathBuf,
    exit_path: PathBuf,
}

impl ToolRegistry {
    pub(super) fn start_command(
        &self,
        command: &str,
        background: bool,
        read_only: bool,
        approved: bool,
        cwd: PathBuf,
    ) -> Result<StartedCommand> {
        let files = self.command_files(background)?;
        let script = if background {
            crate::shell::wrap(command, &files.exit_path, &cwd)
        } else {
            command.to_string()
        };
        let sandbox = self.command_sandbox(approved)?;
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
        if background {
            jobs::write(&files.rec_path, &self.job_record(&files.id, pid, command)?)?;
        }
        Ok(StartedCommand {
            id: files.id,
            pid,
            log_path: files.log_path,
            exit_path: files.exit_path,
            child,
            temp: files.temp,
        })
    }

    fn command_files(&self, background: bool) -> Result<CommandFiles> {
        if background {
            let (id, rec_path, log_path, exit_path) = jobs::prepare(&self.workspace)?;
            return Ok(CommandFiles {
                temp: None,
                id,
                rec_path,
                log_path,
                exit_path,
            });
        }
        let dir = tempfile::tempdir()?;
        Ok(CommandFiles {
            id: String::new(),
            rec_path: dir.path().join("record.json"),
            log_path: dir.path().join("output.log"),
            exit_path: dir.path().join("exit"),
            temp: Some(dir),
        })
    }

    fn command_sandbox(&self, approved: bool) -> Result<Option<Sandbox>> {
        let one_shot = self.sandbox.is_some()
            && self
                .network_once
                .swap(false, std::sync::atomic::Ordering::SeqCst);
        if approved {
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

    fn keep_log(&self, temp: Option<tempfile::TempDir>, keep: bool) {
        if !keep {
            return;
        }
        if let Some(temp) = temp {
            self.keep_dir(temp);
        }
    }

    fn finished(
        &self,
        started: StartedCommand,
        command: &str,
        code: i32,
        read_only: bool,
    ) -> Result<ToolOutput> {
        let StartedCommand {
            log_path,
            exit_path,
            temp,
            ..
        } = started;
        jobs::write_exit(&exit_path, code)?;
        crate::askpass::scrub_file(&log_path);
        let raw = crate::askpass::scrub(&jobs::preview(&log_path, LOG_READ_BYTES)?);
        let grant = (code != 0)
            .then(|| sandbox_grant(self, command, &raw))
            .flatten();
        let shown = digest(command, &raw);
        let output = format!(
            "exit {code}{}\n{}{}{}",
            full_output_note(&log_path, &raw, shown.cut),
            shown.text,
            plan_write_note(read_only, &raw),
            sandbox_note(grant.as_ref(), self, &raw)
        );
        self.keep_log(temp, shown.cut);
        Ok(ToolOutput {
            text: output.trim_end().to_string(),
            success: code == 0,
            mutated: !read_only && code == 0 && !readonly_verb(command),
            grant,
            images: Vec::new(),
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
                Err(error) => detail = format!(" ({error} — could not continue it as a job)"),
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
        let status = started
            .child
            .wait()
            .await
            .map_err(|error| Error::Tool(format!("command cleanup failed: {error}")))?;
        group.disarm();
        let StartedCommand {
            log_path,
            exit_path,
            temp,
            ..
        } = started;
        jobs::write_exit(&exit_path, status.code().unwrap_or(-1))?;
        crate::askpass::scrub_file(&log_path);
        let raw = crate::askpass::scrub(&jobs::preview(&log_path, LOG_READ_BYTES)?);
        let shown = digest(command, &raw);
        let warning = if read_only {
            "workspace writes were restricted by the command sandbox"
        } else {
            "the command may have changed workspace files before timeout"
        };
        self.keep_log(temp, shown.cut);
        Ok(ToolOutput {
            text: format!(
                "{headline}\n{warning}{}\n{}",
                full_output_note(&log_path, &raw, shown.cut),
                shown.text
            )
            .trim_end()
            .to_string(),
            success: false,
            mutated: !read_only,
            grant: None,
            images: Vec::new(),
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
            temp,
            ..
        } = started;
        let raw =
            crate::askpass::scrub(&jobs::preview(&log_path, LOG_READ_BYTES).unwrap_or_default());
        let shown = digest(command, &raw);
        let text = format!(
            "command still running after {seconds}s — it continues as a background job\n{}\n{}",
            job_text(&job.id, pid, &log_path),
            shown.text
        );
        adopt(child, group, job.exit_path, log_path);
        self.keep_log(temp, true);
        Ok(ToolOutput {
            text: text.trim_end().to_string(),
            success: true,
            mutated: !readonly_verb(command),
            grant: None,
            images: Vec::new(),
        })
    }
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
