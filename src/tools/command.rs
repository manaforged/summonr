use std::path::{Path, PathBuf};

use crate::policy::Grant;
use summonr_core::error::{Error, Result};

use super::{ToolOutput, ToolRegistry, object_bool, object_string, object_usize};

pub(super) mod digest;
mod monitor;
mod native;
mod native_search;
pub(super) mod proc;
mod run;

pub(crate) use digest::searchish;
#[cfg(feature = "gui")]
pub(crate) use digest::test_result;
pub(crate) use native::native_read_args;
use native_search::native_search_args;
pub(crate) use proc::{ProcessGroup, command_env, stop_groups};
use proc::{background_result, stop_job};
use run::Wait;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Rewrite {
    Native,
    Exact,
}

pub(crate) fn kill_job(workspace: &Path, id: &str) -> Result<crate::jobs::Status> {
    let record = crate::jobs::load(workspace, id)?;
    stop_job(workspace, &record)?;
    crate::jobs::status(workspace, &record)
}

/// Stops background jobs owned by the given session.
pub(crate) fn stop_jobs(workspace: &Path, session: &str) -> Result<Vec<String>> {
    stop_matching(workspace, session, |_| true)
}

static CHILDREN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Records a child session whose background jobs stop when this process exits.
pub(crate) fn own_child(session: &str) {
    CHILDREN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(session.to_string());
}

/// Stops the background jobs of every child session this process started: every group first, then the records.
pub(crate) fn stop_child_jobs() {
    let children = std::mem::take(
        &mut *CHILDREN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    let jobs: Vec<crate::jobs::Record> = crate::jobs::started_here()
        .into_iter()
        .filter(|record| {
            record
                .session
                .as_ref()
                .is_some_and(|owner| children.contains(owner))
        })
        .collect();
    for record in &jobs {
        if crate::jobs::owner(record.pid, record.started) == crate::jobs::Owner::Job {
            proc::kill_group(record.pid);
        }
    }
    for record in &jobs {
        drop(stop_job(Path::new(&record.workspace), record));
    }
}

impl ToolRegistry {
    pub(super) async fn run_command(&self, args: &serde_json::Value) -> Result<ToolOutput> {
        let named = |key| {
            args.get(key)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.is_empty())
        };
        if args.get("op").and_then(serde_json::Value::as_str) == Some("help")
            && !named("command")
            && !named("job")
        {
            return Ok(ToolOutput::ok(super::catalog::help(
                super::Builtin::RunCommand,
            )));
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
            return self.poll_job(args, id, locked).await;
        }
        self.launch_command(args, locked, Rewrite::Native).await
    }

    pub(crate) async fn run_check(
        &self,
        command: &str,
        timeout_secs: Option<u64>,
    ) -> Result<ToolOutput> {
        let args = serde_json::json!({
            "command": command,
            "timeout_secs": timeout_secs.unwrap_or(crate::limits::limits().verify_timeout_secs),
        });
        self.launch_command(&args, self.policy.is_read_only(), Rewrite::Exact)
            .await
    }

    async fn launch_command(
        &self,
        args: &serde_json::Value,
        locked: bool,
        rewrite: Rewrite,
    ) -> Result<ToolOutput> {
        let restricted = self.policy.restricts_writes();
        let command = object_string(args, "command")?;
        let (background, explicit) = launch_mode(args)?;
        let cwd = self.command_dir(args)?;
        let approved = self.take_approval(command, &cwd)?;
        if let Some(output) = self
            .shortcut(args, command, &cwd, approved, rewrite)
            .await?
        {
            return Ok(output);
        }
        self.check_launch(locked, restricted, background)?;
        let wait = if background {
            Wait::Background
        } else {
            Wait::chosen(
                explicit,
                rewrite == Rewrite::Native && self.can_detach(locked),
            )
        };
        let before = crate::diff::status_records(&self.workspace);
        let watch = (!background).then(|| crate::diff::Watch::start(&self.workspace)).flatten();
        let started = self.start_command(command, wait, cwd)?;
        if background {
            return Ok(background_result(started));
        }
        let output = self.wait_command(started, command, wait).await?;
        let mut paths = crate::diff::changed_paths(&before, &crate::diff::status_records(&self.workspace));
        if let Some(watch) = watch {
            paths.extend(tokio::task::spawn_blocking(move || watch.finish()).await.unwrap_or_default());
        }
        for path in paths {
            self.note_wrote(&path);
        }
        Ok(output)
    }

    async fn shortcut(
        &self,
        args: &serde_json::Value,
        command: &str,
        cwd: &Path,
        approved: bool,
        rewrite: Rewrite,
    ) -> Result<Option<ToolOutput>> {
        if let Some(output) = self.gate_command(command, cwd, approved)? {
            return Ok(Some(output));
        }
        if rewrite == Rewrite::Exact {
            return Ok(None);
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
            None => self.search_natively(command).await,
        }
    }

    pub(super) fn gate_command(
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
                success: false,
                grant: Some(Grant::Danger {
                    command: command.to_string(),
                    reason: reason.to_string(),
                }),
                ..ToolOutput::ok(format!(
                    "summonr: not run; `{command}` needs the user's approval because {reason}"
                ))
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
        if restricted {
            return Err(Error::Denied(
                "a restricted write policy does not run shell commands".to_string(),
            ));
        }
        if locked && background {
            return Err(Error::Denied(
                "A read-only session does not permit background commands".to_string(),
            ));
        }
        if background && !self.background {
            return Err(Error::Denied(
                "the background jobs module is off".to_string(),
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
        if status.state != crate::jobs::JobState::Running {
            self.see_job(id);
        }
        Ok(ToolOutput {
            success: matches!(
                status.state,
                crate::jobs::JobState::Running | crate::jobs::JobState::Exited(0)
            ),
            ..ToolOutput::ok(crate::askpass::scrub(&status.text))
        })
    }

    fn read_natively(&self, command: &str) -> Result<Option<ToolOutput>> {
        let Some((read_args, pipe)) = native_read_args(command) else {
            return Ok(None);
        };
        let mut text = crate::tools::blocking(|| self.read(&read_args.value()))?.text;
        if let Some(pipe) = pipe {
            text = pipe.apply(&text);
        }
        Ok(Some(ToolOutput::ok(text)))
    }

    async fn search_natively(&self, command: &str) -> Result<Option<ToolOutput>> {
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
        let texts =
            futures::future::try_join_all(calls.iter().map(|call| self.search_cancellable(call)))
                .await?;
        let answers: Vec<String> = parts
            .iter()
            .zip(&calls)
            .zip(texts)
            .map(|((part, call), text)| format!("[`{part}` answered by search {call}]\n{text}"))
            .collect();
        Ok(Some(ToolOutput::ok(answers.join("\n\n"))))
    }

    pub(crate) fn stop_jobs(&self) -> Result<Vec<String>> {
        self.stop_matching(|_| true)
    }

    pub(crate) fn stop_detached(&self) -> Result<Vec<String>> {
        let ids = std::mem::take(
            &mut *self
                .detached_jobs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        self.stop_matching(|record| ids.contains(&record.id))
    }

    pub(crate) fn clear_detached(&self) {
        self.detached_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    pub(crate) fn see_job(&self, id: &str) {
        self.seen_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.to_string());
    }

    pub(crate) fn job_seen(&self, id: &str) -> bool {
        self.seen_jobs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(id)
    }

    fn stop_matching(&self, stops: impl Fn(&crate::jobs::Record) -> bool) -> Result<Vec<String>> {
        let session = self
            .session
            .lock()
            .map_err(|error| Error::Tool(format!("cannot read tool session: {error}")))?;
        let Some(session) = session.clone() else {
            return Ok(Vec::new());
        };
        stop_matching(&self.workspace, &session, stops)
    }

    fn check_command(&self, command: &str, cwd: &Path) -> Result<()> {
        self.policy.command(command, &self.workspace, cwd)
    }

    fn check_policy(&self, command: &str, cwd: &Path) -> Result<()> {
        self.policy
            .decide(crate::policy::Action::Command {
                text: command.trim(),
                workspace: &self.workspace,
                cwd,
                ask: false,
                approved: true,
            })
            .into_result()
    }

    pub(super) fn command_dir(&self, args: &serde_json::Value) -> Result<PathBuf> {
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

fn stop_matching(
    workspace: &Path,
    session: &str,
    stops: impl Fn(&crate::jobs::Record) -> bool,
) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    for record in crate::jobs::since(workspace, 0, Some(session))
        .into_iter()
        .filter(|record| record.session.as_deref() == Some(session) && stops(record))
    {
        if crate::jobs::status(workspace, &record)?.state == crate::jobs::JobState::Running {
            let children = stop_job(workspace, &record)?;
            notes.push(format!(
                "killed job {} `{}`\n{}",
                record.id,
                crate::secret::shield::scrub_found(&record.command, workspace),
                crate::jobs::block("child commands at kill", &children)
            ));
        }
    }
    Ok(notes)
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
