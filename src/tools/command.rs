use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::policy::Grant;

use super::sandbox::Sandbox;
use super::{ToolOutput, ToolRegistry, object_bool, object_string, object_usize, parse_args};

pub(super) mod digest;
pub(super) mod proc;
mod rewrite;

use digest::digest_is_lossy;
pub(crate) use digest::searchish;
use proc::{
    ProcessGroup, StartedCommand, background_result, spawn_command, stop_command, stop_job,
    wait_shell,
};
pub(crate) use rewrite::native_read_args;
use rewrite::{plan_write_note, readonly_verb, sandbox_grant, sandbox_note, slow_search_note};

const LOG_READ_BYTES: usize = 512 * 1024;
const TAIL_LINES: usize = 60;

pub(crate) fn exit_code(output: &str) -> Option<i32> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("exit "))
        .and_then(|code| code.trim().parse().ok())
}

impl ToolRegistry {
    pub(super) async fn run_command(&self, arguments: &str) -> Result<ToolOutput> {
        let args: serde_json::Value = parse_args("run_command", arguments)?;
        if args.get("op").and_then(serde_json::Value::as_str) == Some("help") {
            return Ok(ToolOutput::ok(super::help::RUN_COMMAND.to_string()));
        }
        let plan = self.policy.is_read_only();
        let restricted = self.policy.restricts_writes();
        let read_only = plan || restricted;
        if let Some(id) = args
            .get("job")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
        {
            if plan {
                return Err(Error::Denied(
                    "Plan mode does not control background commands".to_string(),
                ));
            }
            if object_bool(&args, "kill") {
                let record = crate::jobs::load(&self.workspace, id)?;
                let workspace = self.workspace.clone();
                tokio::task::spawn_blocking(move || stop_job(&workspace, &record))
                    .await
                    .map_err(|error| Error::Tool(format!("job stop failed: {error}")))??;
            }
            let status = crate::jobs::status(&self.workspace, id)?;
            return Ok(ToolOutput {
                success: status.running || status.exit_code == Some(0),
                text: crate::askpass::scrub(&status.text),
                mutated: false,
                grant: None,
            });
        }
        let command = object_string(&args, "command")?;
        if let Some((read_args, pipe)) = native_read_args(command) {
            let mut text = tokio::task::block_in_place(|| self.read(&read_args.to_string()))?;
            if let Some(pipe) = pipe {
                text = pipe.apply(&text);
            }
            text.push_str("\nnote: `read` answers this natively — call it directly next time");
            return Ok(ToolOutput {
                success: true,
                text,
                mutated: false,
                grant: None,
            });
        }
        let approved = self.take_approval();
        if !approved
            && self.policy.is_unrestricted()
            && let Some(reason) = crate::policy::danger(command, &self.workspace)
        {
            return Ok(ToolOutput {
                text: format!(
                    "summonr: not run — `{command}` needs the user's approval because {reason}"
                ),
                success: false,
                mutated: false,
                grant: Some(Grant::Danger(command.to_string())),
            });
        }
        if approved {
            self.check_policy(command)?;
        } else {
            self.check_command(command)?;
        }
        let background = object_bool(&args, "background");
        if restricted && self.sandbox.is_none() {
            return Err(Error::Denied(
                "a restricted write policy requires the command sandbox for shell inspection"
                    .to_string(),
            ));
        }
        if read_only && background {
            return Err(Error::Denied(
                if plan {
                    "Plan mode does not permit background commands"
                } else {
                    "restricted write policy does not permit background commands"
                }
                .to_string(),
            ));
        }
        if background && !self.background {
            return Err(Error::Denied(
                "background commands are unavailable in child sessions".to_string(),
            ));
        }
        let cwd = self.command_dir(&args)?;
        let started = self.start_command(command, background, read_only, approved, cwd)?;
        if background {
            return Ok(background_result(started));
        }
        let timeout = object_usize(&args, "timeout_secs")
            .and_then(|secs| u64::try_from(secs).ok())
            .or(Some(crate::runtime::JOB_WAIT_SECS));
        self.wait_command(started, command, timeout, read_only, !approved)
            .await
    }

    pub(crate) fn stop_jobs(&self) -> Result<()> {
        let session = self
            .session
            .lock()
            .map_err(|error| Error::Tool(format!("cannot read tool session: {error}")))?;
        let Some(session) = session.clone() else {
            return Ok(());
        };
        for record in crate::jobs::since(&self.workspace, 0, None)
            .into_iter()
            .filter(|record| record.session.as_deref() == Some(&session))
        {
            if crate::jobs::status(&self.workspace, &record.id)?.running {
                stop_job(&self.workspace, &record)?;
            }
        }
        Ok(())
    }

    fn check_command(&self, command: &str) -> Result<()> {
        self.policy.command(command, &self.workspace)
    }

    fn check_policy(&self, command: &str) -> Result<()> {
        self.policy.command_policy_checked(
            command.trim(),
            &crate::policy::split_segments(command.trim()),
        )
    }

    fn command_dir(&self, args: &serde_json::Value) -> Result<PathBuf> {
        let Some(dir) = args
            .get("workdir")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|dir| !dir.is_empty())
        else {
            return Ok(self.workspace.clone());
        };
        let path = self.resolve_read(dir)?;
        if !path.is_dir() {
            return Err(Error::Tool(format!("`{dir}` is not a directory")));
        }
        Ok(path)
    }

    fn start_command(
        &self,
        command: &str,
        background: bool,
        read_only: bool,
        approved: bool,
        cwd: PathBuf,
    ) -> Result<StartedCommand> {
        let temp = if background {
            None
        } else {
            Some(tempfile::tempdir()?)
        };
        let job_workspace = temp
            .as_ref()
            .map_or(self.workspace.as_path(), tempfile::TempDir::path);
        let (id, rec_path, log_path, exit_path) = match temp.as_ref() {
            Some(dir) => (
                String::new(),
                dir.path().join("record.json"),
                dir.path().join("output.log"),
                dir.path().join("exit"),
            ),
            None => crate::jobs::prepare(job_workspace)?,
        };
        let script = if background {
            crate::shell::wrap(command, &exit_path, &cwd)
        } else {
            command.to_string()
        };
        let one_shot = self.sandbox.is_some()
            && self
                .network_once
                .swap(false, std::sync::atomic::Ordering::SeqCst);
        let sandbox = if approved {
            None
        } else if one_shot {
            self.sandbox
                .as_ref()
                .map(Sandbox::allow_network)
                .transpose()?
        } else {
            self.sandbox.clone()
        };
        let child = spawn_command(
            &self.env,
            sandbox.as_ref(),
            &script,
            &log_path,
            background,
            read_only,
            &cwd,
        )?;
        let pid = child
            .id()
            .ok_or_else(|| Error::Tool("command started without a pid".to_string()))?;
        if background {
            let mut record = crate::jobs::stamp(
                &id,
                pid,
                command,
                &self.workspace,
                true,
                self.session.lock().ok().and_then(|session| session.clone()),
            )?;
            record.sandbox = self.sandbox.is_some();
            crate::jobs::write(&rec_path, &record)?;
        }
        Ok(StartedCommand {
            id,
            pid,
            log_path,
            exit_path,
            child,
            temp,
        })
    }

    fn full_output_note(&self, log_path: &Path, raw: &str) -> String {
        if !digest_is_lossy(raw) {
            return String::new();
        }
        format!(
            "\n[full output: {} — {} lines]",
            log_path.display(),
            raw.lines().count()
        )
    }

    fn keep_log(&self, temp: Option<tempfile::TempDir>, raw: &str) {
        if !digest_is_lossy(raw) {
            return;
        }
        if let Some(temp) = temp
            && let Ok(mut logs) = self.logs.lock()
        {
            logs.push(temp);
        }
    }

    async fn wait_command(
        &self,
        started: StartedCommand,
        command: &str,
        timeout: Option<u64>,
        read_only: bool,
        contained: bool,
    ) -> Result<ToolOutput> {
        let StartedCommand {
            id: _,
            pid,
            log_path,
            exit_path,
            mut child,
            temp,
        } = started;
        let mut group = ProcessGroup::new(pid, contained);
        let started = std::time::Instant::now();
        let waited = match timeout {
            Some(seconds) => {
                tokio::time::timeout(crate::limits::seconds(seconds), wait_shell(&mut child)).await
            }
            None => Ok(wait_shell(&mut child).await),
        };
        match waited {
            Ok(Ok(status)) => {
                group.disarm();
                let code = status.code().unwrap_or(-1);
                crate::jobs::write_exit(&exit_path, code)?;
                crate::askpass::scrub_file(&log_path);
                let raw = crate::askpass::scrub(&crate::jobs::preview(&log_path, LOG_READ_BYTES)?);
                let grant = (code != 0)
                    .then(|| sandbox_grant(self, command, &raw))
                    .flatten();
                let full = self.full_output_note(&log_path, &raw);
                let output = format!(
                    "exit {code}{full}\n{}{}{}{}",
                    super::digest(command, &raw),
                    slow_search_note(command, started.elapsed().as_secs()),
                    plan_write_note(read_only, &raw),
                    sandbox_note(grant.as_ref(), self, &raw)
                );
                self.keep_log(temp, &raw);
                Ok(ToolOutput {
                    text: output.trim_end().to_string(),
                    success: code == 0,
                    mutated: !read_only && code == 0 && !readonly_verb(command),
                    grant,
                })
            }
            Ok(Err(error)) => Err(Error::Tool(format!("command failed: {error}"))),
            Err(_) => {
                stop_command(&mut child, pid)
                    .map_err(|error| Error::Tool(format!("command cleanup failed: {error}")))?;
                let status = child
                    .wait()
                    .await
                    .map_err(|error| Error::Tool(format!("command cleanup failed: {error}")))?;
                group.disarm();
                crate::jobs::write_exit(&exit_path, status.code().unwrap_or(-1))?;
                crate::askpass::scrub_file(&log_path);
                let raw = crate::askpass::scrub(&crate::jobs::preview(&log_path, LOG_READ_BYTES)?);
                let full = self.full_output_note(&log_path, &raw);
                let timeout = timeout.map_or_else(
                    || "command timed out".to_string(),
                    |seconds| {
                        format!(
                            "command timed out after {seconds}s — retry with a larger timeout_secs or run it with background true"
                        )
                    },
                );
                let warning = if read_only {
                    "workspace writes were restricted by the command sandbox"
                } else {
                    "the command may have changed workspace files before timeout"
                };
                self.keep_log(temp, &raw);
                Ok(ToolOutput {
                    text: format!(
                        "{timeout}\n{warning}{full}\n{}",
                        super::digest(command, &raw)
                    )
                    .trim_end()
                    .to_string(),
                    success: false,
                    mutated: !read_only,
                    grant: None,
                })
            }
        }
    }
}
