use std::ffi::OsString;
use std::path::{Path, PathBuf};
#[cfg(sandboxed)]
use std::process::Stdio;

use tokio::process::Command;

#[cfg(sandboxed)]
use crate::error::Error;
use crate::error::Result;

#[cfg(all(windows, sandboxed))]
pub(crate) mod windows;

#[cfg(target_os = "macos")]
mod macos;

mod discover;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sandbox {
    #[cfg(sandboxed)]
    backend: Backend,
    #[cfg(sandboxed)]
    workspace: PathBuf,
    #[cfg(sandboxed)]
    deny_network: bool,
    #[cfg(sandboxed)]
    write: Vec<PathBuf>,
    #[cfg(target_os = "linux")]
    temp: PathBuf,
}

#[cfg(sandboxed)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum Backend {
    #[cfg(all(windows, sandboxed))]
    Windows { executable: PathBuf },
    #[cfg(target_os = "macos")]
    Mac {
        executable: PathBuf,
        profile: String,
        read_profile: String,
    },
    #[cfg(target_os = "linux")]
    Linux { executable: PathBuf },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    Allowed,
    Denied,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandMode {
    ReadOnly,
    Session,
    Background,
}

impl Network {
    #[must_use]
    pub const fn allowed(allow: bool) -> Self {
        if allow { Self::Allowed } else { Self::Denied }
    }
}

impl Sandbox {
    pub(crate) fn discover(workspace: &Path, network: Network, write: &[PathBuf]) -> Result<Self> {
        discover::discover(workspace, network == Network::Denied, write)
    }

    pub(crate) fn at(&self, workspace: &Path) -> Result<Self> {
        #[cfg(sandboxed)]
        {
            Self::discover(workspace, Network::allowed(!self.deny_network), &self.write)
        }
        #[cfg(not(sandboxed))]
        {
            let _ = self;
            Self::discover(workspace, Network::Allowed, &[])
        }
    }

    pub(crate) fn allow_network(&self) -> Result<Self> {
        #[cfg(sandboxed)]
        {
            Self::discover(&self.workspace, Network::Allowed, &self.write)
        }
        #[cfg(not(sandboxed))]
        {
            let _ = self;
            unreachable!("sandbox backend is unavailable")
        }
    }

    pub(crate) fn denies_network(&self) -> bool {
        #[cfg(sandboxed)]
        {
            self.deny_network
        }
        #[cfg(not(sandboxed))]
        {
            false
        }
    }

    #[cfg(sandboxed)]
    fn command_line(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
        mode: CommandMode,
    ) -> (PathBuf, Vec<OsString>) {
        let read_only = mode == CommandMode::ReadOnly;
        #[cfg(target_os = "macos")]
        let _ = cwd;
        let (executable, args) = match &self.backend {
            #[cfg(all(windows, sandboxed))]
            Backend::Windows { executable } => {
                let mut command = vec![
                    "--sandbox-exec".into(),
                    self.workspace.as_os_str().to_owned(),
                    cwd.as_os_str().to_owned(),
                    read_only.to_string().into(),
                    self.deny_network.to_string().into(),
                    self.write
                        .iter()
                        .map(|path| path.to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join(";")
                        .into(),
                    std::process::id().to_string().into(),
                    program.into(),
                ];
                command.extend(args.iter().map(|arg| (*arg).into()));
                (executable.clone(), command)
            }
            #[cfg(target_os = "macos")]
            Backend::Mac {
                executable,
                profile,
                read_profile,
            } => {
                let profile = if read_only { read_profile } else { profile };
                let mut command = vec!["-p".into(), profile.clone().into(), program.into()];
                command.extend(args.iter().map(|arg| (*arg).into()));
                (executable.clone(), command)
            }
            #[cfg(target_os = "linux")]
            Backend::Linux { executable } => {
                let mut command = vec![
                    "--ro-bind".into(),
                    "/".into(),
                    "/".into(),
                    "--dev-bind".into(),
                    "/dev/null".into(),
                    "/dev/null".into(),
                    "--bind".into(),
                    self.temp.as_os_str().to_os_string(),
                    self.temp.as_os_str().to_os_string(),
                ];
                if read_only {
                    command.extend([
                        "--ro-bind".into(),
                        self.workspace.as_os_str().to_os_string(),
                        self.workspace.as_os_str().to_os_string(),
                    ]);
                } else {
                    command.extend([
                        "--bind".into(),
                        self.workspace.as_os_str().to_os_string(),
                        self.workspace.as_os_str().to_os_string(),
                    ]);
                }
                for path in &self.write {
                    command.extend([
                        "--bind".into(),
                        path.as_os_str().to_os_string(),
                        path.as_os_str().to_os_string(),
                    ]);
                }
                for path in crate::policy::guarded_git_paths(&self.workspace) {
                    if !read_only && path.exists() {
                        command.extend([
                            "--ro-bind".into(),
                            path.as_os_str().to_os_string(),
                            path.as_os_str().to_os_string(),
                        ]);
                    }
                }
                command.extend([
                    "--new-session".into(),
                    "--unshare-pid".into(),
                    "--proc".into(),
                    "/proc".into(),
                ]);
                if mode != CommandMode::Background {
                    command.push("--die-with-parent".into());
                }
                if self.deny_network {
                    command.push("--unshare-net".into());
                }
                command.push("--chdir".into());
                command.push(cwd.as_os_str().to_os_string());
                command.push(program.into());
                command.extend(args.iter().map(|arg| (*arg).into()));
                (executable.clone(), command)
            }
        };
        (executable, args)
    }

    #[cfg(not(sandboxed))]
    fn command_line(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
        mode: CommandMode,
    ) -> (PathBuf, Vec<OsString>) {
        let _ = (self, program, args, cwd, mode);
        unreachable!("sandbox backend is unavailable")
    }

    #[cfg(sandboxed)]
    fn probe(&self) -> Result<()> {
        let shell = crate::shell::program()?;
        let args = crate::shell::arguments("exit 0", &self.workspace);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let (executable, args) =
            self.command_line(&shell, &args, &self.workspace, CommandMode::Session);
        let status = std::process::Command::new(executable)
            .args(args)
            .current_dir(&self.workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|error| {
                Error::Config(format!("--sandbox backend failed to start: {error}"))
            })?;
        if status.success() {
            return Ok(());
        }
        Err(Error::Config(format!(
            "--sandbox backend preflight failed with {status}"
        )))
    }

    pub(crate) fn command(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
        mode: CommandMode,
    ) -> Command {
        let (executable, args) = self.command_line(program, args, cwd, mode);
        let mut command = Command::new(executable);
        command.args(args);
        command.current_dir(cwd);
        command
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|path| is_executable(path))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        path.metadata()
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    true
}
