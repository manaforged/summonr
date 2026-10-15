use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[cfg(sandboxed)]
use super::Backend;
use super::Sandbox;

#[cfg(not(sandboxed))]
pub(super) fn discover(workspace: &Path, deny_network: bool, write: &[PathBuf]) -> Result<Sandbox> {
    let _ = (workspace, deny_network, write);
    Err(Error::Config(
        "--sandbox needs macOS Seatbelt, Linux bubblewrap, or Windows".to_string(),
    ))
}

#[cfg(sandboxed)]
pub(super) fn discover(workspace: &Path, deny_network: bool, write: &[PathBuf]) -> Result<Sandbox> {
    let workspace = workspace
        .canonicalize()
        .map_err(|error| Error::Config(format!("cannot open sandbox workspace: {error}")))?;
    let write = write
        .iter()
        .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
        .collect::<Vec<PathBuf>>();
    let sandbox = build(workspace, deny_network, write)?;
    sandbox.probe()?;
    Ok(sandbox)
}

#[cfg(all(sandboxed, any(target_os = "macos", target_os = "linux")))]
fn temp_dir() -> Result<PathBuf> {
    std::env::temp_dir().canonicalize().map_err(|error| {
        Error::Config(format!(
            "cannot open the system temp directory for sandbox: {error}"
        ))
    })
}

#[cfg(all(windows, sandboxed))]
fn build(workspace: PathBuf, deny_network: bool, write: Vec<PathBuf>) -> Result<Sandbox> {
    if deny_network {
        return Err(Error::Config(
            super::windows::NETWORK_UNSUPPORTED.to_string(),
        ));
    }
    let executable = std::env::current_exe()?;
    #[cfg(test)]
    let executable = executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| Error::Config("cannot locate the sandbox test executable".to_string()))?
        .join("summonr.exe");
    Ok(Sandbox {
        backend: Backend::Windows { executable },
        workspace,
        deny_network,
        write,
    })
}

#[cfg(all(sandboxed, target_os = "macos"))]
fn build(workspace: PathBuf, deny_network: bool, write: Vec<PathBuf>) -> Result<Sandbox> {
    let temp = temp_dir()?;
    let executable = super::find_in_path("sandbox-exec").ok_or_else(|| {
        Error::Config(
            "--sandbox requires macOS sandbox-exec (Seatbelt), but it is not available".to_string(),
        )
    })?;
    let profile = super::macos::profile(&workspace, &temp, &write, deny_network, false)?;
    let read_profile = super::macos::profile(&workspace, &temp, &write, deny_network, true)?;
    Ok(Sandbox {
        backend: Backend::Mac {
            executable,
            profile,
            read_profile,
        },
        workspace,
        deny_network,
        write,
    })
}

#[cfg(all(sandboxed, target_os = "linux"))]
fn build(workspace: PathBuf, deny_network: bool, write: Vec<PathBuf>) -> Result<Sandbox> {
    let temp = temp_dir()?;
    let executable = super::find_in_path("bwrap")
        .or_else(|| super::find_in_path("bubblewrap"))
        .ok_or_else(|| {
            Error::Config(
                "--sandbox requires Linux bubblewrap (bwrap), but it is not available".to_string(),
            )
        })?;
    Ok(Sandbox {
        backend: Backend::Linux { executable },
        workspace,
        temp,
        deny_network,
        write,
    })
}
