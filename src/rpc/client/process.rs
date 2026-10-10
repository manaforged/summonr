use std::path::Path;
use std::process::Stdio;

use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

use crate::error::{Error, Result};
use crate::tools::command::ProcessGroup;
use crate::tools::sandbox::{CommandMode, Sandbox};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Env {
    Clean,
    Inherit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Write,
    Read,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stderr {
    Pipe,
    Discard,
}

impl Access {
    pub(crate) fn from_read_only(read_only: bool) -> Self {
        if read_only { Self::Read } else { Self::Write }
    }
}

pub(crate) struct Launch<'a> {
    pub(crate) program: &'a str,
    pub(crate) args: &'a [&'a str],
    pub(crate) env: &'a [(String, String)],
    pub(crate) cwd: &'a Path,
    pub(crate) sandbox: Option<(&'a Sandbox, Access)>,
    pub(crate) base: Env,
    pub(crate) stderr: Stderr,
}

pub(crate) struct Process {
    pub(crate) child: Child,
    pub(crate) stdin: ChildStdin,
    pub(crate) stdout: ChildStdout,
    pub(crate) stderr: Option<ChildStderr>,
    pub(crate) group: Option<ProcessGroup>,
}

pub(crate) fn start(launch: &Launch<'_>) -> Result<Process> {
    let mut command = match launch.sandbox {
        Some((sandbox, access)) => sandbox.command(
            launch.program,
            launch.args,
            launch.cwd,
            match access {
                Access::Read => CommandMode::ReadOnly,
                Access::Write => CommandMode::Session,
            },
        ),
        None => {
            let mut command = Command::new(launch.program);
            command.args(launch.args).current_dir(launch.cwd);
            command
        }
    };
    #[cfg(unix)]
    command.process_group(0);
    if launch.base == Env::Clean {
        command
            .env_clear()
            .envs(crate::tools::command::command_env());
    }
    command.envs(launch.env.iter().map(|(key, value)| (key, value)));
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(match launch.stderr {
            Stderr::Pipe => Stdio::piped(),
            Stderr::Discard => Stdio::null(),
        })
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| Error::Tool(format!("cannot start `{}`: {error}", launch.program)))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::Tool("child has no stdin".to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Tool("child has no stdout".to_string()))?;
    let stderr = child.stderr.take();
    let group = child
        .id()
        .map(|pid| ProcessGroup::new(pid, launch.sandbox.is_some()));
    Ok(Process {
        child,
        stdin,
        stdout,
        stderr,
        group,
    })
}

#[cfg(all(test, unix))]
#[path = "process_tests.rs"]
mod tests;
