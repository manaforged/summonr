use std::path::Path;
use std::process::{Command, Stdio};

use super::Stat;
use crate::error::{Error, Result};

pub(super) fn numstat_row(line: &str) -> Option<Stat> {
    let mut parts = line.splitn(3, '\t');
    let added = parts.next()?.trim();
    let removed = parts.next()?.trim();
    let path = parts.next()?.trim();
    if path.is_empty() {
        return None;
    }
    Some(Stat {
        path: path.to_string(),
        added: added.parse().unwrap_or(0),
        removed: removed.parse().unwrap_or(0),
        new: false,
    })
}

pub(super) fn git(root: &Path, args: &[&str]) -> Result<String> {
    git_env(root, &[], args)
}

pub(crate) fn git_env(
    root: &Path,
    env: &[(&str, &std::ffi::OsStr)],
    args: &[&str],
) -> Result<String> {
    Ok(String::from_utf8_lossy(&git_output(root, env, args, None)?)
        .trim()
        .to_string())
}

pub(crate) fn git_output(
    root: &Path,
    env: &[(&str, &std::ffi::OsStr)],
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<Vec<u8>> {
    use std::io::Write;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .envs(env.iter().copied())
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(input)?;
    }
    let output = child.wait_with_output()?;
    let different = args.starts_with(&["diff", "--no-index"])
        && output.status.code() == Some(1)
        && !output.stdout.is_empty();
    if !output.status.success() && !different {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Config(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            stderr.trim()
        )));
    }
    Ok(output.stdout)
}

pub(crate) fn git_paths(root: &Path, args: &[&str]) -> Result<Vec<String>> {
    let out = git_output(root, &[], args, None)?;
    Ok(String::from_utf8_lossy(&out)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}
