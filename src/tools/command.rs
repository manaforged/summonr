use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::policy::Grant;

use super::{ToolOutput, ToolRegistry, object_bool, object_string, object_usize, parse_args};

pub(super) mod digest;
mod native;
pub(super) mod proc;
mod rewrite;
mod run;

pub(crate) use digest::searchish;
pub(crate) use native::native_read_args;
pub(crate) use proc::ProcessGroup;
use proc::{background_result, stop_job};
use run::Wait;

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
        if args.get("op").and_then(serde_json::Value::as_str) == Some("help") {
            return Ok(ToolOutput::ok(super::help::RUN_COMMAND.to_string()));
        }
        let plan = self.policy.is_read_only();
        if let Some(id) = args
            .get("job")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
        {
            return self.poll_job(&args, id, plan).await;
        }
        self.launch_command(&args, plan).await
    }

    async fn launch_command(&self, args: &serde_json::Value, plan: bool) -> Result<ToolOutput> {
        let restricted = self.policy.restricts_writes();
        let read_only = plan || restricted;
        let command = object_string(args, "command")?;
        let workdir = args
            .get("workdir")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|dir| !dir.trim().is_empty());
        if !workdir && let Some(output) = self.read_natively(command)? {
            return Ok(output);
        }
        let approved = self.take_approval();
        if let Some(output) = self.gate_command(command, approved)? {
            return Ok(output);
        }
        let background = object_bool(args, "background");
        self.check_launch(plan, restricted, background)?;
        let cwd = self.command_dir(args)?;
        let started = self.start_command(command, background, read_only, approved, cwd)?;
        if background {
            return Ok(background_result(started));
        }
        let explicit = object_usize(args, "timeout_secs").and_then(|secs| u64::try_from(secs).ok());
        let wait = Wait::chosen(explicit, self.can_detach(read_only));
        self.wait_command(started, command, wait, read_only, !approved)
            .await
    }

    fn gate_command(&self, command: &str, approved: bool) -> Result<Option<ToolOutput>> {
        if !approved
            && self.policy.is_unrestricted()
            && let Some(reason) = crate::policy::danger(command, &self.workspace)
        {
            return Ok(Some(ToolOutput {
                text: format!(
                    "summonr: not run — `{command}` needs the user's approval because {reason}"
                ),
                success: false,
                mutated: false,
                grant: Some(Grant::Danger(command.to_string())),
                images: Vec::new(),
                patch: None,
            }));
        }
        if approved {
            self.check_policy(command)?;
        } else {
            self.check_command(command)?;
        }
        Ok(None)
    }

    fn check_launch(&self, plan: bool, restricted: bool, background: bool) -> Result<()> {
        if restricted && self.sandbox.is_none() {
            return Err(Error::Denied(
                "a restricted write policy requires the command sandbox for shell inspection"
                    .to_string(),
            ));
        }
        if (plan || restricted) && background {
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
        Ok(())
    }

    async fn poll_job(&self, args: &serde_json::Value, id: &str, plan: bool) -> Result<ToolOutput> {
        if plan {
            return Err(Error::Denied(
                "Plan mode does not control background commands".to_string(),
            ));
        }
        if object_bool(args, "kill") {
            let record = crate::jobs::load(&self.workspace, id)?;
            let workspace = self.workspace.clone();
            tokio::task::spawn_blocking(move || stop_job(&workspace, &record))
                .await
                .map_err(|error| Error::Tool(format!("job stop failed: {error}")))??;
        }
        let status =
            crate::jobs::wait(
                &self.workspace,
                id,
                crate::runtime::JOB_WAIT_SECS,
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
}
