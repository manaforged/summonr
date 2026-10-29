use std::ffi::{OsStr, OsString};
use std::io::{Read, Write as _};
use std::path::PathBuf;
use std::process::{Output, Stdio};
use std::time::{Duration, Instant};

use tokio::process::{ChildStderr, ChildStdin, ChildStdout};

use crate::tools::command::ProcessGroup;
use summonr_core::error::{Error, Result};

mod info;
mod tail;

pub(crate) use info::{command, footprint_kb, parent, started, table};
pub(crate) use tail::{Cap, Tail};

const SECRET_MIN: usize = 8;
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Env {
    Clean,
    Inherit,
}

#[derive(Debug)]
pub(crate) enum Io {
    Pipe,
    Null,
    File(std::fs::File),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    Own,
    Detached,
}

#[derive(Debug)]
pub(crate) struct Spec {
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
    pub(crate) env: Vec<(OsString, OsString)>,
    pub(crate) remove: Vec<OsString>,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) base: Env,
    pub(crate) stdin: Io,
    pub(crate) stdout: Io,
    pub(crate) stderr: Io,
    pub(crate) group: Group,
    pub(crate) kill_on_drop: bool,
}

impl Spec {
    pub(crate) fn new<A: Into<OsString>>(
        program: impl Into<OsString>,
        args: impl IntoIterator<Item = A>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: Vec::new(),
            remove: Vec::new(),
            cwd: None,
            base: Env::Inherit,
            stdin: Io::Pipe,
            stdout: Io::Pipe,
            stderr: Io::Pipe,
            group: Group::Own,
            kill_on_drop: true,
        }
    }

    #[must_use]
    pub(crate) fn envs<K: Into<OsString>, V: Into<OsString>>(
        mut self,
        pairs: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        self.env.extend(
            pairs
                .into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        self
    }

    fn command(self) -> std::process::Command {
        let mut command = std::process::Command::new(&self.program);
        command.args(&self.args);
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        if self.base == Env::Clean {
            command
                .env_clear()
                .envs(crate::tools::command::command_env());
        }
        for key in &self.remove {
            command.env_remove(key);
        }
        command
            .envs(self.env)
            .stdin(stdio(self.stdin))
            .stdout(stdio(self.stdout))
            .stderr(stdio(self.stderr));
        #[cfg(unix)]
        match self.group {
            Group::Own | Group::Detached => {
                std::os::unix::process::CommandExt::process_group(&mut command, 0);
            }
        }
        #[cfg(windows)]
        if self.group == Group::Detached {
            std::os::windows::process::CommandExt::creation_flags(
                &mut command,
                DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
            );
        }
        command
    }
}

fn stdio(io: Io) -> Stdio {
    match io {
        Io::Pipe => Stdio::piped(),
        Io::Null => Stdio::null(),
        Io::File(file) => Stdio::from(file),
    }
}

fn failed(program: &OsStr, error: &std::io::Error) -> Error {
    Error::Tool(format!(
        "cannot start `{}`: {error}",
        program.to_string_lossy()
    ))
}

#[cfg(not(windows))]
pub(crate) type Child = tokio::process::Child;

#[cfg(windows)]
#[derive(Debug)]
pub(crate) struct Child {
    pub(crate) inner: Box<dyn process_wrap::tokio::ChildWrapper>,
    kill_on_drop: bool,
}

#[cfg(windows)]
impl Child {
    pub(crate) fn id(&self) -> Option<u32> {
        self.inner.id()
    }

    pub(crate) async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.inner.wait().await
    }

    pub(crate) fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.inner.try_wait()
    }

    pub(crate) fn start_kill(&mut self) -> std::io::Result<()> {
        self.inner.start_kill()
    }

    pub(crate) async fn kill(&mut self) -> std::io::Result<()> {
        Box::into_pin(self.inner.kill()).await
    }
}

#[cfg(windows)]
impl Drop for Child {
    fn drop(&mut self) {
        if self.kill_on_drop
            && let Err(error) = self.start_kill()
        {
            eprintln!("command cleanup failed: {error}");
        }
    }
}

pub(crate) fn spawn(spec: Spec) -> Result<Child> {
    let program = spec.program.clone();
    #[cfg(windows)]
    let group = spec.group;
    let kill_on_drop = spec.kill_on_drop;
    let mut command = tokio::process::Command::from(spec.command());
    command.kill_on_drop(kill_on_drop);
    #[cfg(not(windows))]
    let child = command.spawn();
    #[cfg(windows)]
    let child = {
        let mut command = process_wrap::tokio::CommandWrap::from(command);
        if group == Group::Own {
            command.wrap(process_wrap::tokio::JobObject);
        }
        command.spawn().map(|inner| Child {
            inner,
            kill_on_drop,
        })
    };
    child.map_err(|error| failed(&program, &error))
}

pub(crate) fn spawn_blocking(spec: Spec) -> Result<std::process::Child> {
    let program = spec.program.clone();
    spec.command()
        .spawn()
        .map_err(|error| failed(&program, &error))
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Deadline {
    pub(crate) after: Duration,
    pub(crate) poll: Duration,
}

pub(crate) fn output(spec: Spec, input: Option<&[u8]>) -> Result<Output> {
    Ok(fed(spec, input)?.wait_with_output()?)
}

pub(crate) fn output_within(
    spec: Spec,
    input: Option<&[u8]>,
    deadline: Deadline,
) -> Result<Option<Output>> {
    let mut child = fed(spec, input)?;
    let (closed, wake) = std::sync::mpsc::channel();
    let stdout = drain(child.stdout.take(), closed.clone());
    let stderr = drain(child.stderr.take(), closed.clone());
    let until = Instant::now() + deadline.after;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= until {
            drop(child.kill());
            drop(child.wait());
            return Ok(None);
        }
        wake.recv_timeout(deadline.poll).unwrap_or(());
    };
    Ok(Some(Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    }))
}

fn fed(mut spec: Spec, input: Option<&[u8]>) -> Result<std::process::Child> {
    spec.stdin = if input.is_some() { Io::Pipe } else { Io::Null };
    spec.stdout = Io::Pipe;
    spec.stderr = Io::Pipe;
    let mut child = spawn_blocking(spec)?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(input)?;
    }
    Ok(child)
}

fn drain(
    pipe: Option<impl Read + Send + 'static>,
    closed: std::sync::mpsc::Sender<()>,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            drop(pipe.read_to_end(&mut bytes));
        }
        closed.send(()).unwrap_or(());
        bytes
    })
}

pub(crate) fn pipes(
    child: &mut Child,
) -> (Option<ChildStdin>, Option<ChildStdout>, Option<ChildStderr>) {
    #[cfg(not(windows))]
    {
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    }
    #[cfg(windows)]
    {
        (
            child.inner.stdin().take(),
            child.inner.stdout().take(),
            child.inner.stderr().take(),
        )
    }
}

pub(crate) struct Process {
    pub(crate) child: Child,
    pub(crate) stdin: ChildStdin,
    pub(crate) stdout: ChildStdout,
    pub(crate) stderr: Option<ChildStderr>,
    pub(crate) group: Option<ProcessGroup>,
}

pub(crate) fn start(spec: Spec) -> Result<Process> {
    let mut child = spawn(spec)?;
    let (stdin, stdout, stderr) = pipes(&mut child);
    let stdin = stdin.ok_or_else(|| Error::Tool("child has no stdin".to_string()))?;
    let stdout = stdout.ok_or_else(|| Error::Tool("child has no stdout".to_string()))?;
    let group = child.id().map(ProcessGroup::new);
    Ok(Process {
        child,
        stdin,
        stdout,
        stderr,
        group,
    })
}
