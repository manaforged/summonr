use std::ffi::OsStr;
use std::process::Command;

use crate::{Error, Result};

const HEADLESS: &str = "SUMMONR_NO_BROWSER";

pub fn external(target: impl AsRef<OsStr>) -> Result<()> {
    if cfg!(test) || std::env::var_os(HEADLESS).is_some() {
        return Err(Error::Setup(format!("{HEADLESS} is set")));
    }
    let (program, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("open", &[])
    } else if cfg!(windows) {
        ("cmd", &["/C", "start", ""])
    } else {
        ("xdg-open", &[])
    };
    let status = Command::new(program)
        .args(args)
        .arg(target)
        .status()
        .map_err(|error| Error::Setup(format!("cannot start {program}: {error}")))?;
    if !status.success() {
        return Err(Error::Setup(format!("{program} exited with {status}")));
    }
    Ok(())
}
