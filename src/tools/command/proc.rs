use std::path::PathBuf;
#[cfg(windows)]
use std::process::Stdio;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::error::{Error, Result};
use crate::process::{Env, Io, Spec};

use super::ToolOutput;

pub(super) struct StartedCommand {
    pub(super) id: String,
    pub(super) pid: u32,
    pub(super) log_path: PathBuf,
    pub(super) child: CommandChild,
    pub(super) group: ProcessGroup,
}

pub(super) type CommandChild = crate::process::Child;

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
    let path = command_env()
        .into_iter()
        .find_map(|(key, value)| (key == "PATH").then_some(value))
        .unwrap_or_default();
    crate::process::spawn(
        Spec {
            cwd: Some(cwd.to_path_buf()),
            base: Env::Clean,
            stdin: Io::Null,
            stdout: Io::File(log),
            stderr: Io::File(err),
            kill_on_drop: !background,
            ..Spec::new(
                crate::shell::program()?,
                crate::shell::arguments(script, cwd),
            )
        }
        .envs(crate::askpass::env(&path))
        .envs(env.iter().cloned()),
    )
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
    ToolOutput::ok(job_text(&id, pid, &log_path))
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
        child.inner.inner_mut().wait().await
    }
}

pub(super) fn stop_job(
    workspace: &std::path::Path,
    record: &crate::jobs::Record,
) -> Result<Vec<String>> {
    let exit = crate::jobs::exit_path(workspace, &record.id);
    let mut children = Vec::new();
    if crate::jobs::alive_since(record.pid, record.started) {
        children = crate::jobs::child_lines(record.pid, workspace);
        kill_group(record.pid);
    }
    if !exit.is_file() {
        crate::jobs::write_killed(&exit, &children)?;
    }
    Ok(children)
}
