use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::policy::Grant;

use super::{ToolOutput, ToolRegistry, object_bool, object_string, object_usize, parse_args};

pub(super) mod digest;
mod native;
mod native_search;
pub(super) mod proc;
mod rewrite;
mod run;

pub(crate) use digest::searchish;
pub(crate) use native::native_read_args;
use native_search::native_search_args;
pub(crate) use proc::{ProcessGroup, command_env, stop_groups};
use proc::{background_result, stop_job};
use run::Wait;

#[derive(Clone, Copy)]
pub(crate) enum Approval {
    Policy,
    Uncontained,
}

pub(crate) fn exit_code(output: &str) -> Option<i32> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("exit "))
        .and_then(|code| code.trim().parse().ok())
}

pub(crate) fn kill_job(workspace: &Path, id: &str) -> Result<crate::jobs::Status> {
    let record = crate::jobs::load(workspace, id)?;
    stop_job(workspace, &record)?;
    crate::jobs::status(workspace, &record)
}

impl ToolRegistry {
    pub(super) async fn run_command(&self, arguments: &str) -> Result<ToolOutput> {
        let args: serde_json::Value = parse_args("run_command", arguments)?;
        let named = |key| {
            args.get(key)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.is_empty())
        };
        if args.get("op").and_then(serde_json::Value::as_str) == Some("help")
            && !named("command")
            && !named("job")
        {
            return Ok(ToolOutput::ok(super::help::RUN_COMMAND.to_string()));
        }
        let locked = self.policy.is_read_only();
        if let Some(id) = args
            .get("job")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
        {
            if named("command") {
                return Err(Error::Tool(format!(
                    "run_command got both job `{id}` and a command; send job to poll or stop it, or command to start one"
                )));
            }
            return self.poll_job(&args, id, locked).await;
        }
        self.launch_command(&args, locked).await
    }

    async fn launch_command(&self, args: &serde_json::Value, locked: bool) -> Result<ToolOutput> {
        let restricted = self.policy.restricts_writes();
        let read_only = locked || restricted;
        let command = object_string(args, "command")?;
        let (background, explicit) = launch_mode(args)?;
        let approval = self.take_approval()?;
        let cwd = self.command_dir(args)?;
        if let Some(output) = self.shortcut(args, command, &cwd, approval.is_some())? {
            return Ok(output);
        }
        self.check_launch(locked, restricted, background)?;
        let wait = if background {
            Wait::Background
        } else {
            Wait::chosen(explicit, self.can_detach(read_only))
        };
        let started = self.start_command(command, wait, read_only, approval, cwd)?;
        if background {
            return Ok(background_result(started));
        }
        self.wait_command(started, command, wait, read_only).await
    }

    fn shortcut(
        &self,
        args: &serde_json::Value,
        command: &str,
        cwd: &Path,
        approved: bool,
    ) -> Result<Option<ToolOutput>> {
        if let Some(output) = self.gate_command(command, cwd, approved)? {
            return Ok(Some(output));
        }
        let workdir = args
            .get("workdir")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|dir| !dir.trim().is_empty());
        if workdir {
            return Ok(None);
        }
        match self.read_natively(command)? {
            Some(output) => Ok(Some(output)),
            None => self.search_natively(command),
        }
    }

    fn gate_command(
        &self,
        command: &str,
        cwd: &Path,
        approved: bool,
    ) -> Result<Option<ToolOutput>> {
        if !approved
            && !self.policy.is_strict()
            && let Some(reason) = self.policy.needs_approval(command, &self.workspace, cwd)
        {
            return Ok(Some(ToolOutput {
                text: format!(
                    "summonr: not run; `{command}` needs the user's approval because {reason}"
                ),
                success: false,
                mutated: false,
                grant: Some(Grant::Danger {
                    command: command.to_string(),
                    reason: reason.to_string(),
                }),
                images: Vec::new(),
                patch: None,
            }));
        }
        if approved {
            self.check_policy(command, cwd)?;
        } else {
            self.check_command(command, cwd)?;
        }
        Ok(None)
    }

    fn check_launch(&self, locked: bool, restricted: bool, background: bool) -> Result<()> {
        if restricted && self.sandbox.is_none() {
            return Err(Error::Denied(
                "a restricted write policy requires the command sandbox for shell inspection"
                    .to_string(),
            ));
        }
        if (locked || restricted) && background {
            return Err(Error::Denied(
                if locked {
                    "A read-only session does not permit background commands"
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
        Ok(())
    }

    async fn poll_job(
        &self,
        args: &serde_json::Value,
        id: &str,
        locked: bool,
    ) -> Result<ToolOutput> {
        if locked {
            return Err(Error::Denied(
                "A read-only session does not control background commands".to_string(),
            ));
        }
        if object_bool(args, "kill") {
            let record = crate::jobs::load(&self.workspace, id)?;
            let workspace = self.workspace.clone();
            tokio::task::spawn_blocking(move || stop_job(&workspace, &record))
                .await
                .map_err(|error| Error::Tool(format!("job stop failed: {error}")))??;
        }
        let status = crate::jobs::wait(
            &self.workspace,
            id,
            crate::limits::limits().job_wait_secs,
            || Ok(()),
        )
        .await?;
        Ok(ToolOutput {
            success: matches!(
                status.state,
                crate::jobs::JobState::Running | crate::jobs::JobState::Exited(0)
            ),
            text: crate::askpass::scrub(&status.text),
            mutated: false,
            grant: None,
            images: Vec::new(),
            patch: None,
        })
    }

    fn read_natively(&self, command: &str) -> Result<Option<ToolOutput>> {
        let Some((read_args, pipe)) = native_read_args(command) else {
            return Ok(None);
        };
        let mut text = crate::tools::blocking(|| self.read(&read_args.to_json()))?.text;
        if let Some(pipe) = pipe {
            text = pipe.apply(&text);
        }
        Ok(Some(ToolOutput::ok(text)))
    }

    fn search_natively(&self, command: &str) -> Result<Option<ToolOutput>> {
        let parts: Vec<&str> = command
            .split(';')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();
        let Some(calls) = parts
            .iter()
            .map(|part| native_search_args(part))
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(None);
        };
        let mut answers = Vec::with_capacity(calls.len());
        for (part, call) in parts.iter().zip(calls) {
            let text = crate::tools::blocking(|| self.search_files(&call.to_string()))?;
            answers.push(format!("[`{part}` answered by search {call}]\n{text}"));
        }
        Ok(Some(ToolOutput::ok(answers.join("\n\n"))))
    }

    pub(crate) fn stop_jobs(&self) -> Result<()> {
        let session = self
            .session
            .lock()
            .map_err(|error| Error::Tool(format!("cannot read tool session: {error}")))?;
        let Some(session) = session.clone() else {
            return Ok(());
        };
        for record in crate::jobs::since(&self.workspace, 0, Some(&session))
            .into_iter()
            .filter(|record| record.session.as_deref() == Some(&session))
        {
            if crate::jobs::status(&self.workspace, &record)?.state
                == crate::jobs::JobState::Running
            {
                stop_job(&self.workspace, &record)?;
            }
        }
        Ok(())
    }

    fn check_command(&self, command: &str, cwd: &Path) -> Result<()> {
        self.policy.command(command, &self.workspace, cwd)
    }

    fn check_policy(&self, command: &str, cwd: &Path) -> Result<()> {
        self.policy.command_policy_checked(
            command.trim(),
            &crate::policy::split_segments(command.trim()),
            &self.workspace,
            cwd,
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
        let path = self.resolve_read(dir, crate::tools::paths::Shield::Contained)?;
        if !path.is_dir() {
            return Err(Error::Tool(format!("`{dir}` is not a directory")));
        }
        Ok(path)
    }
}

fn launch_mode(args: &serde_json::Value) -> Result<(bool, Option<u64>)> {
    let background = object_bool(args, "background");
    let explicit = object_usize(args, "timeout_secs")
        .filter(|secs| *secs > 0)
        .and_then(|secs| u64::try_from(secs).ok());
    if background && explicit.is_some() {
        return Err(Error::Tool(
            "timeout_secs applies to foreground commands; drop it or background, then stop a job with run_command {\"job\":ID,\"kill\":true}".to_string(),
        ));
    }
    Ok((background, explicit))
}
