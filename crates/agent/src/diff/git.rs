use std::ffi::OsStr;
use std::path::Path;

use super::Stat;
use crate::process::{self, Spec};
use summonr_core::error::{Error, Result};

const UNATTENDED: [(&str, &str); 4] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_CONFIG_COUNT", "1"),
    ("GIT_CONFIG_KEY_0", "protocol.ext.allow"),
    ("GIT_CONFIG_VALUE_0", "never"),
];

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
    let spec = git_spec(root, args).envs(env.iter().copied());
    checked(args, process::output(spec, input)?)
}

/// Runs `git -C root ARGS` with no prompt and no `ext::` transport, as a timeout past `deadline`.
pub(crate) fn git_within(
    root: &Path,
    args: &[&str],
    deadline: process::Deadline,
) -> Result<Vec<u8>> {
    let spec = git_spec(root, args).envs(UNATTENDED);
    let Some(output) = process::output_within(spec, None, deadline)? else {
        return Err(Error::Timeout {
            what: format!("git {}", args.first().unwrap_or(&"")),
            limit: deadline.after,
        });
    };
    checked(args, output)
}

fn git_spec(root: &Path, args: &[&str]) -> Spec {
    let argv = [OsStr::new("-C"), root.as_os_str()]
        .into_iter()
        .chain(args.iter().map(OsStr::new));
    Spec::new("git", argv)
}

fn checked(args: &[&str], output: std::process::Output) -> Result<Vec<u8>> {
    let different = args.starts_with(&["diff", "--no-index"])
        && output.status.code() == Some(1)
        && !output.stdout.is_empty();
    if !output.status.success() && !different {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        return Err(Error::Config(format!(
            "git {} failed: {detail}",
            args.first().unwrap_or(&"")
        )));
    }
    Ok(output.stdout)
}

pub(crate) fn git_paths(root: &Path, args: &[&str]) -> Result<Vec<String>> {
    Ok(nul_paths(&git_output(root, &[], args, None)?))
}

pub(crate) fn nul_paths(out: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(out)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn git_apply(root: &Path, patch: &[u8], flags: &[&str]) -> Result<()> {
    let mut args = vec!["apply", "--binary", "--whitespace=nowarn"];
    args.extend(flags);
    git_output(root, &[], &args, Some(patch))?;
    Ok(())
}
