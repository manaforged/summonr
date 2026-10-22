use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Mutex, MutexGuard, PoisonError};

#[cfg(windows)]
use process_wrap::tokio::{ChildWrapper, CommandWrap, JobObject};
#[cfg(unix)]
use tokio::process::Child;
use tokio::process::Command;

use crate::error::{Error, Result};

use super::ToolOutput;

pub(super) struct StartedCommand {
    pub(super) id: String,
    pub(super) pid: u32,
    pub(super) log_path: PathBuf,
    pub(super) child: CommandChild,
    pub(super) group: ProcessGroup,
}

#[cfg(unix)]
pub(super) type CommandChild = Child;

#[cfg(windows)]
pub(super) struct CommandChild {
    child: Box<dyn ChildWrapper>,
    kill_on_drop: bool,
}

#[cfg(windows)]
impl CommandChild {
    pub(super) fn id(&self) -> Option<u32> {
        self.child.id()
    }

    pub(super) async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    fn start_kill(&mut self) -> std::io::Result<()> {
        self.child.start_kill()
    }
}

#[cfg(windows)]
impl Drop for CommandChild {
    fn drop(&mut self) {
        if self.kill_on_drop
            && let Err(error) = self.start_kill()
        {
            eprintln!("command cleanup failed: {error}");
        }
    }
}

pub(crate) struct ProcessGroup {
    pid: u32,
    armed: bool,
}

static GROUPS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

fn groups() -> MutexGuard<'static, Vec<u32>> {
    GROUPS.lock().unwrap_or_else(PoisonError::into_inner)
}

impl ProcessGroup {
    pub(crate) fn new(pid: u32) -> Self {
        groups().push(pid);
        Self { pid, armed: true }
    }

    pub(crate) fn terminate(&self) -> bool {
        #[cfg(unix)]
        {
            signal(self.pid, libc::SIGTERM)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        let mut live = groups();
        if let Some(at) = live.iter().position(|group| *group == self.pid) {
            live.swap_remove(at);
        }
        drop(live);
        if self.armed {
            kill_group(self.pid);
        }
    }
}

pub(crate) fn stop_groups() {
    let taken = std::mem::take(&mut *groups());
    for pid in taken {
        kill_group(pid);
    }
}

pub(super) fn spawn_command(
    env: &[(String, String)],
    script: &str,
    log_path: &std::path::Path,
    background: bool,
    cwd: &std::path::Path,
) -> Result<CommandChild> {
    let log = std::fs::File::create(log_path)
        .map_err(|error| Error::Tool(format!("cannot create job log: {error}")))?;
    let err = log
        .try_clone()
        .map_err(|error| Error::Tool(format!("cannot clone job log: {error}")))?;
    let shell = crate::shell::program()?;
    let args = crate::shell::arguments(script, cwd);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let base = command_env();
    let path = base
        .iter()
        .find_map(|(key, value)| (key == "PATH").then(|| value.clone()))
        .unwrap_or_default();
    let mut cmd = Command::new(&shell);
    cmd.args(&args)
        .current_dir(cwd)
        .env_clear()
        .envs(base)
        .envs(crate::askpass::env(&path))
        .envs(env.iter().cloned())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .kill_on_drop(!background)
        .stdin(Stdio::null());
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    #[cfg(unix)]
    let child = cmd.spawn();
    #[cfg(windows)]
    let child = {
        let mut command = CommandWrap::from(cmd);
        command.wrap(JobObject);
        command.spawn().map(|child| CommandChild {
            child,
            kill_on_drop: !background,
        })
    };
    child.map_err(|error| Error::Tool(format!("cannot start command: {error}")))
}

pub(super) fn background_result(started: StartedCommand) -> ToolOutput {
    let StartedCommand {
        id,
        pid,
        log_path,
        child,
        mut group,
        ..
    } = started;
    group.disarm();
    drop(child);
    ToolOutput {
        text: job_text(&id, pid, &log_path),
        success: true,
        mutated: false,
        grant: None,
        images: Vec::new(),
        patch: None,
    }
}

pub(super) fn job_text(id: &str, pid: u32, log_path: &std::path::Path) -> String {
    format!(
        "started job {id}\npid {pid}\nlog {}\nstate: {}\nwait for it: run_command {{\"job\":\"{id}\"}}",
        log_path.display(),
        crate::jobs::JobState::Running
    )
}

pub(crate) fn command_env() -> Vec<(String, String)> {
    const KEEP: &[&str] = &[
        "PATH",
        "HOME",
        "TMPDIR",
        "TMP",
        "TEMP",
        "LANG",
        "LC_ALL",
        "TERM",
        "SHELL",
        "USER",
        "LOGNAME",
        "SSH_AUTH_SOCK",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "CARGO_TARGET_DIR",
        crate::cron::runner::ENV,
        "SystemRoot",
        "ComSpec",
        "PATHEXT",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "ProgramData",
        "ALLUSERSPROFILE",
        "PSModulePath",
        "windir",
        "SystemDrive",
        "COMPUTERNAME",
        "USERNAME",
        "PROCESSOR_ARCHITECTURE",
        "NUMBER_OF_PROCESSORS",
        "OS",
    ];
    let mut env: Vec<_> = KEEP
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_string(), value))
        })
        .collect();
    if let Some(extra) = crate::plugins::bin_path_prefix() {
        let sep = if cfg!(windows) { ';' } else { ':' };
        match env.iter_mut().find(|(key, _)| key == "PATH") {
            Some((_, path)) => {
                if !path.is_empty() {
                    *path = format!("{extra}{sep}{path}");
                } else {
                    *path = extra;
                }
            }
            None => env.push(("PATH".to_string(), extra)),
        }
    }
    env
}

pub(in crate::tools) fn kill_group(pid: u32) {
    #[cfg(unix)]
    signal(pid, libc::SIGKILL);
    #[cfg(windows)]
    drop(
        std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    );
}

#[cfg(unix)]
#[expect(unsafe_code, reason = "std has no process-group signal")]
fn signal(group: u32, signal: libc::c_int) -> bool {
    let Ok(group) = libc::pid_t::try_from(group) else {
        return false;
    };
    // SAFETY: killpg reads two integers and touches no memory.
    unsafe { libc::killpg(group, signal) == 0 }
}

pub(super) fn stop_command(child: &mut CommandChild, pid: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let _ = child;
        kill_group(pid);
        Ok(())
    }
    #[cfg(windows)]
    {
        let _ = pid;
        child.start_kill()
    }
}

pub(super) async fn wait_shell(
    child: &mut CommandChild,
) -> std::io::Result<std::process::ExitStatus> {
    #[cfg(unix)]
    {
        child.wait().await
    }
    #[cfg(windows)]
    {
        child.child.inner_mut().wait().await
    }
}

pub(super) fn stop_job(workspace: &std::path::Path, record: &crate::jobs::Record) -> Result<()> {
    let exit = crate::jobs::exit_path(workspace, &record.id);
    if crate::jobs::alive_since(record.pid, record.started) {
        kill_group(record.pid);
    }
    if !exit.is_file() {
        crate::jobs::write_killed(&exit)?;
    }
    Ok(())
}
