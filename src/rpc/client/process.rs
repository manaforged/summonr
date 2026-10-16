use std::path::Path;
use std::process::Stdio;

use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

use crate::error::{Error, Result};
use crate::tools::command::ProcessGroup;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Env {
    Clean,
    Inherit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stderr {
    Pipe,
    Discard,
}

pub(crate) struct Launch<'a> {
    pub(crate) program: &'a str,
    pub(crate) args: &'a [&'a str],
    pub(crate) env: &'a [(String, String)],
    pub(crate) unset: &'a [&'a str],
    pub(crate) cwd: &'a Path,
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
    let mut command = Command::new(launch.program);
    command.args(launch.args).current_dir(launch.cwd);
    #[cfg(unix)]
    command.process_group(0);
    if launch.base == Env::Clean {
        command
            .env_clear()
            .envs(crate::tools::command::command_env());
    }
    for key in launch.unset {
        command.env_remove(key);
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
    let group = child.id().map(ProcessGroup::new);
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
