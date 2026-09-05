use std::ffi::OsString;
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::Stdio;

use tokio::process::Command;

use crate::error::{Error, Result};

#[derive(Clone, Debug)]
pub(crate) struct Sandbox {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    backend: Backend,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    workspace: PathBuf,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    deny_network: bool,
    #[cfg(target_os = "linux")]
    temp: PathBuf,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[derive(Clone, Debug)]
enum Backend {
    #[cfg(target_os = "macos")]
    Mac {
        executable: PathBuf,
        profile: String,
        read_profile: String,
    },
    #[cfg(target_os = "linux")]
    Linux { executable: PathBuf },
}

impl Sandbox {
    pub(crate) fn discover(workspace: &Path, deny_network: bool) -> Result<Self> {
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = (workspace, deny_network);
            Err(Error::Config(
                "--sandbox is unsupported on this platform; use macOS Seatbelt or Linux bubblewrap"
                    .to_string(),
            ))
        }

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let workspace = workspace.canonicalize().map_err(|error| {
                Error::Config(format!("cannot open sandbox workspace: {error}"))
            })?;
            let temp = std::env::temp_dir().canonicalize().map_err(|error| {
                Error::Config(format!(
                    "cannot open the system temp directory for sandbox: {error}"
                ))
            })?;

            #[cfg(target_os = "macos")]
            {
                let executable = find_in_path("sandbox-exec").ok_or_else(|| {
                    Error::Config(
                        "--sandbox requires macOS sandbox-exec (Seatbelt), but it is not available"
                            .to_string(),
                    )
                })?;
                let profile = mac_profile(&workspace, &temp, deny_network, false)?;
                let read_profile = mac_profile(&workspace, &temp, deny_network, true)?;
                let sandbox = Self {
                    backend: Backend::Mac {
                        executable,
                        profile,
                        read_profile,
                    },
                    workspace,
                    deny_network,
                };
                sandbox.probe()?;
                Ok(sandbox)
            }

            #[cfg(target_os = "linux")]
            {
                let executable = find_in_path("bwrap")
                    .or_else(|| find_in_path("bubblewrap"))
                    .ok_or_else(|| {
                        Error::Config(
                            "--sandbox requires Linux bubblewrap (bwrap), but it is not available"
                                .to_string(),
                        )
                    })?;
                let sandbox = Self {
                    backend: Backend::Linux { executable },
                    workspace,
                    temp,
                    deny_network,
                };
                sandbox.probe()?;
                Ok(sandbox)
            }
        }
    }

    pub(crate) fn at(&self, workspace: &Path) -> Result<Self> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            Self::discover(workspace, self.deny_network)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = self;
            Self::discover(workspace, false)
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub(crate) fn command_line(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
    ) -> (PathBuf, Vec<OsString>) {
        self.command_line_with(program, args, cwd, false)
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub(crate) fn read_command_line(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
    ) -> (PathBuf, Vec<OsString>) {
        self.command_line_with(program, args, cwd, true)
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn command_line_with(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
        read_only: bool,
    ) -> (PathBuf, Vec<OsString>) {
        #[cfg(target_os = "macos")]
        let _ = cwd;
        let (executable, args) = match &self.backend {
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

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub(crate) fn command_line(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
    ) -> (PathBuf, Vec<OsString>) {
        let _ = (self, program, args, cwd);
        unreachable!("sandbox backend is unavailable")
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    pub(crate) fn read_command_line(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
    ) -> (PathBuf, Vec<OsString>) {
        self.command_line(program, args, cwd)
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn probe(&self) -> Result<()> {
        let (executable, args) = self.command_line("true", &[], &self.workspace);
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

    pub(crate) fn command(&self, program: &str, args: &[&str], cwd: &Path) -> Command {
        let (executable, args) = self.command_line(program, args, cwd);
        let mut command = Command::new(executable);
        command.args(args);
        command.current_dir(cwd);
        command
    }

    pub(crate) fn read_command(&self, program: &str, args: &[&str], cwd: &Path) -> Command {
        let (executable, args) = self.read_command_line(program, args, cwd);
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

#[cfg(target_os = "macos")]
fn mac_profile(
    workspace: &Path,
    temp: &Path,
    deny_network: bool,
    read_only: bool,
) -> Result<String> {
    let workspace = profile_path(workspace)?;
    let temp = profile_path(temp)?;
    let network_rule = if deny_network {
        String::from("(deny network*)\n")
    } else {
        String::new()
    };
    let (workspace_rule, workspace_deny) = if read_only {
        (
            String::new(),
            format!("(deny file-write* (subpath \"{workspace}\"))\n"),
        )
    } else {
        (format!("    (subpath \"{workspace}\")\n"), String::new())
    };
    Ok(format!(
        "(version 1)\n\
(allow default)\n\
{network_rule}\
(deny file-write*)\n\
(allow file-write*\n\
    (literal \"/dev/null\")\n\
{workspace_rule}\
    (subpath \"{temp}\")\n\
 )\n\
{workspace_deny}"
    ))
}

#[cfg(target_os = "macos")]
fn profile_path(path: &Path) -> Result<String> {
    let path = path.to_str().ok_or_else(|| {
        Error::Config(format!(
            "sandbox path is not valid UTF-8: {}",
            path.display()
        ))
    })?;
    Ok(path.replace('\\', "\\\\").replace('"', "\\\""))
}
