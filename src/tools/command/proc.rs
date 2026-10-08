use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;

#[cfg(windows)]
use process_wrap::tokio::{ChildWrapper, CommandWrap, JobObject};
#[cfg(unix)]
use tokio::process::Child;
use tokio::process::Command;

use crate::error::{Error, Result};

use super::ToolOutput;
use crate::tools::sandbox::Sandbox;

pub(super) struct StartedCommand {
    pub(super) id: String,
    pub(super) pid: u32,
    pub(super) log_path: PathBuf,
    pub(super) child: CommandChild,
}

#[cfg(unix)]
pub(super) type CommandChild = Child;

#[cfg(windows)]
pub(super) struct CommandChild {
    child: Box<dyn ChildWrapper>,
    kill_on_drop: bool,
    sandbox: bool,
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
        #[cfg(sandboxed)]
        if self.sandbox {
            return self.child.id().map_or(Ok(()), |pid| {
                crate::tools::sandbox::windows::stop(pid).map(drop)
            });
        }
        #[cfg(not(sandboxed))]
        let _ = self.sandbox;
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
    sandbox: bool,
}

fn live_groups() -> &'static Mutex<Vec<(u32, bool)>> {
    static GROUPS: Mutex<Vec<(u32, bool)>> = Mutex::new(Vec::new());
    &GROUPS
}

fn with_groups(mut apply: impl FnMut(&mut Vec<(u32, bool)>)) {
    match live_groups().lock() {
        Ok(mut groups) => apply(&mut groups),
        Err(poisoned) => apply(&mut poisoned.into_inner()),
    }
}

impl ProcessGroup {
    pub(crate) fn new(pid: u32, sandbox: bool) -> Self {
        with_groups(|groups| groups.push((pid, sandbox)));
        Self {
            pid,
            armed: true,
            sandbox,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        let pid = self.pid;
        with_groups(|groups| {
            if let Some(at) = groups.iter().position(|group| group.0 == pid) {
                groups.swap_remove(at);
            }
        });
        if self.armed {
            kill_group(self.pid, self.sandbox);
        }
    }
}

pub(crate) fn stop_groups() {
    let taken = match live_groups().lock() {
        Ok(mut groups) => std::mem::take(&mut *groups),
        Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
    };
    for (pid, sandbox) in taken {
        kill_group(pid, sandbox);
    }
}

pub(super) fn spawn_command(
    env: &[(String, String)],
    sandbox: Option<&Sandbox>,
    script: &str,
    log_path: &std::path::Path,
    background: bool,
    read_only: bool,
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
    let mut cmd = if let Some(sandbox) = sandbox {
        if read_only {
            sandbox.read_command(&shell, &args, cwd)
        } else {
            sandbox.command(&shell, &args, cwd)
        }
    } else {
        let mut cmd = Command::new(&shell);
        cmd.args(&args).current_dir(cwd);
        cmd
    };
    cmd.env_clear()
        .envs(command_env())
        .envs(crate::askpass::env())
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
    let child = if background || sandbox.is_some() {
        cmd.kill_on_drop(false);
        if background && sandbox.is_some() {
            cmd.env("SUMMONR_SANDBOX_DETACHED", "1");
        }
        cmd.spawn().map(|child| CommandChild {
            child: Box::new(child),
            kill_on_drop: !background,
            sandbox: sandbox.is_some(),
        })
    } else {
        let mut command = CommandWrap::from(cmd);
        command.wrap(JobObject);
        command.spawn().map(|child| CommandChild {
            child,
            kill_on_drop: true,
            sandbox: false,
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
        ..
    } = started;
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
        "CARGO_HOME",
        "RUSTUP_HOME",
        "CARGO_TARGET_DIR",
        "SystemRoot",
        "ComSpec",
        "PATHEXT",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
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

pub(in crate::tools) fn kill_group(pid: u32, sandbox: bool) {
    #[cfg(all(windows, sandboxed))]
    if sandbox {
        if let Err(error) = crate::tools::sandbox::windows::stop(pid) {
            eprintln!("sandbox cleanup failed: {error}");
        }
        return;
    }
    #[cfg(not(all(windows, sandboxed)))]
    let _ = sandbox;
    #[cfg(unix)]
    if signal(pid, libc::SIGTERM) {
        let escalate = async move {
            tokio::time::sleep(crate::limits::limits().job_wait() / 60).await;
            let _ = signal(pid, 0) && signal(pid, libc::SIGKILL);
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(runtime.spawn(escalate)),
            Err(error) => {
                eprintln!("command cleanup escalates now: {error}");
                signal(pid, libc::SIGKILL);
            }
        }
    }
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
        kill_group(pid, false);
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
        #[cfg(all(windows, sandboxed))]
        if record.sandbox {
            if let Some(helper) = crate::tools::sandbox::windows::stop(record.pid)? {
                crate::tools::sandbox::windows::wait(&helper)?;
            }
        } else {
            kill_group(record.pid, false);
        }
        #[cfg(not(all(windows, sandboxed)))]
        kill_group(record.pid, record.sandbox);
    }
    if !exit.is_file() {
        crate::jobs::write_killed(&exit)?;
    }
    Ok(())
}
