use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

mod danger;
pub(crate) use danger::{argv, danger, verb};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Policy {
    pub strict: bool,
    read_only: bool,
    unrestricted: bool,
    deny: Vec<String>,
    allow: Vec<Vec<String>>,
    write: Vec<Vec<String>>,
    roots: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Grant {
    Write(PathBuf),
    Guarded(PathBuf),
    Command(String),
    Danger(String),
    Edit(&'static str),
    Network,
}

impl Grant {
    #[must_use]
    pub(crate) fn prompt(&self) -> String {
        match self {
            Self::Write(path) => path.display().to_string(),
            Self::Guarded(path) => format!("protected path {}", path.display()),
            Self::Command(command) => format!("a shell command without the sandbox: {command}"),
            Self::Danger(command) => format!("a destructive command: {command}"),
            Self::Edit(setting) => format!("allow {setting} for this task (saved setting is off)"),
            Self::Network => "network".to_string(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct File {
    #[serde(default)]
    strict: bool,
    #[serde(default)]
    deny: Vec<String>,
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    write: Vec<String>,
}

impl Policy {
    #[must_use]
    pub(crate) fn builtin() -> Self {
        Self::default()
    }

    pub(crate) fn discover(workspace: &Path) -> Result<Self> {
        let mut policy = Self::builtin();
        if let Ok(home) = crate::prompt::summonr_home() {
            policy.overlay_file(&home.join("policy.toml"))?;
        }
        policy.overlay_file(&workspace.join(".summonr/policy.toml"))?;
        policy.roots = crate::config::file()?.sandbox.paths();
        Ok(policy)
    }

    fn overlay_file(&mut self, path: &Path) -> Result<()> {
        if !path.is_file() {
            return Ok(());
        }
        let text = std::fs::read_to_string(path)?;
        let file: File = toml::from_str(&text)
            .map_err(|error| Error::Config(format!("{}: {error}", path.display())))?;
        self.overlay(file);
        Ok(())
    }

    fn overlay(&mut self, file: File) {
        self.strict |= file.strict;
        for item in file.deny {
            if !item.trim().is_empty() && !self.deny.iter().any(|have| have == &item) {
                self.deny.push(item);
            }
        }
        if file.strict || !file.allow.is_empty() {
            self.allow.push(file.allow);
        }
        if !file.write.is_empty() {
            self.write.push(file.write);
        }
    }

    pub(crate) fn allow_path(&mut self, path: PathBuf) {
        let resolved = path.canonicalize().unwrap_or(path);
        if !self.roots.contains(&resolved) {
            self.roots.push(resolved);
        }
    }

    #[must_use]
    pub(crate) fn write_roots(&self) -> Vec<PathBuf> {
        self.roots.clone()
    }

    pub(crate) fn set_strict(&mut self) {
        self.unrestricted = false;
        self.strict = true;
    }

    pub(crate) fn set_unrestricted(&mut self) {
        self.unrestricted = true;
    }

    #[must_use]
    pub(crate) fn is_unrestricted(&self) -> bool {
        self.unrestricted
    }

    #[must_use]
    pub(crate) fn restricts_writes(&self) -> bool {
        !self.unrestricted && !self.write.is_empty()
    }

    pub(crate) fn set_read_only(&mut self, on: bool) {
        self.read_only = on;
    }

    #[must_use]
    pub(crate) fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) fn command(&self, command: &str, workspace: &Path) -> Result<()> {
        let command = command.trim();
        let segments = split_segments(command);
        self.command_policy_checked(command, &segments)?;
        if self.unrestricted {
            return Ok(());
        }
        match write_block_segments(&segments, workspace, &self.roots) {
            Some(Block::Protected(path)) => {
                return Err(Error::Denied(format!("`{path}` is a protected path")));
            }
            Some(Block::Outside(path)) => {
                return Err(Error::Denied(format!(
                    "`{path}` is outside the workspace — {}",
                    outside_hint(&path)
                )));
            }
            None => {}
        }
        Ok(())
    }

    pub(crate) fn command_policy_checked(&self, command: &str, segments: &[String]) -> Result<()> {
        if command.is_empty() {
            return Err(Error::Denied("command is empty".to_string()));
        }
        if self.unrestricted {
            return Ok(());
        }
        if builtin_deny(command, segments) {
            return Err(Error::Denied(format!(
                "blocked command `{command}` — summonr never runs it; run it yourself in another terminal"
            )));
        }
        for pattern in &self.deny {
            if matches_deny(command, pattern) {
                return Err(Error::Denied(format!("denied by policy `{pattern}`")));
            }
        }
        if (self.strict && self.allow.is_empty())
            || !segments.iter().all(|segment| {
                !(self.strict && segment.contains(['`', '$', '<', '>'].as_ref()))
                    && (self.allow.is_empty()
                        || !["$(", "`", "<(", ">("]
                            .iter()
                            .any(|token| segment.contains(token)))
                    && self
                        .allow
                        .iter()
                        .all(|list| list.iter().any(|pattern| matches_allow(segment, pattern)))
            })
        {
            return Err(Error::Denied(format!(
                "command `{command}` is not on every configured allow list"
            )));
        }
        Ok(())
    }

    pub(crate) fn write_path(&self, relative: &str) -> Result<()> {
        if self.read_only {
            return Err(Error::Denied(
                "plan mode does not edit files. Return the plan in your reply. \
                 Switch to auto mode before editing files."
                    .to_string(),
            ));
        }
        if !self.unrestricted && protected_write(Path::new(relative)) {
            return Err(Error::Denied(format!("`{relative}` is a protected path")));
        }
        self.write_globs(relative)
    }

    pub(crate) fn write_globs(&self, relative: &str) -> Result<()> {
        if self.unrestricted {
            return Ok(());
        }
        if self.write.is_empty() {
            return Ok(());
        }
        let relative = relative.replace('\\', "/");
        if self
            .write
            .iter()
            .all(|list| list.iter().any(|glob| path_matches(&relative, glob)))
        {
            return Ok(());
        }
        Err(Error::Denied(format!(
            "write to `{relative}` is not permitted by the policy write globs"
        )))
    }
}

#[must_use]
pub(crate) fn write_directory(path: &str) -> PathBuf {
    let trimmed = path.trim_end_matches(['/', '\\']);
    let directory =
        trimmed.rsplit_once(['/', '\\']).map_or(
            trimmed,
            |(head, _)| {
                if head.is_empty() { "/" } else { head }
            },
        );
    expand_path(directory)
}

#[must_use]
pub(crate) fn write_denied(path: &str) -> String {
    let dir = write_directory(path);
    format!(
        "write to `{path}` is outside the writable roots — add {} to [sandbox] write in ~/.summonr/config.toml, or /permissions write {}",
        dir.display(),
        dir.display()
    )
}

#[must_use]
pub(crate) fn outside_hint(path: &str) -> String {
    write_denied(path)
}

pub(crate) fn grant_from_error(error: &Error) -> Option<Grant> {
    match error {
        Error::EditApproval { setting, .. } => Some(Grant::Edit(setting)),
        Error::Denied(message) | Error::Tool(message) => grant_from_message(message),
        _ => None,
    }
}

pub(crate) fn grant_from_message(message: &str) -> Option<Grant> {
    let rest = message.split_once("/permissions write ")?.1;
    let dir = rest.split([',', '\n']).next()?.trim();
    if dir.is_empty() || dir == "PATH" {
        return None;
    }
    write_grant(expand_path(dir))
}

pub(crate) fn outside_write(command: &str, workspace: &Path, roots: &[PathBuf]) -> Option<Grant> {
    match write_block_segments(&split_segments(command.trim()), workspace, roots) {
        Some(Block::Outside(path)) => write_grant(write_directory(&path)),
        Some(Block::Protected(path)) => {
            write_grant(write_directory(&locate(&path, workspace).to_string_lossy()))
        }
        None => None,
    }
}

fn write_grant(path: PathBuf) -> Option<Grant> {
    if !(path.is_absolute() || path.has_root()) {
        return None;
    }
    Some(if protected_write(&path) {
        Grant::Guarded(path)
    } else {
        Grant::Write(path)
    })
}

pub(crate) fn implicit_write(command: &str) -> Option<PathBuf> {
    let verb = first_verb(command)?;
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    let dir = match verb.as_str() {
        "cargo" | "rustup" => home.join(".cargo"),
        "npm" | "npx" | "pnpm" | "yarn" | "bun" => home.join(".npm"),
        "go" => std::env::var_os("GOPATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("go")),
        _ => return None,
    };
    (!protected_write(&dir)).then_some(dir)
}

pub(crate) fn first_verb(command: &str) -> Option<String> {
    let word = command
        .split_whitespace()
        .find(|word| !word.contains('='))?;
    let name = word
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(word)
        .trim_matches(['\'', '"', '`'])
        .to_ascii_lowercase();
    Some(name.strip_suffix(".exe").unwrap_or(&name).to_string())
}

pub(crate) fn protected_write(path: &Path) -> bool {
    let unix = path.to_string_lossy().replace('\\', "/");
    let parts: Vec<&str> = unix.split('/').filter(|part| !part.is_empty()).collect();
    if parts
        .windows(2)
        .any(|pair| pair[0] == ".git" && matches!(pair[1], "hooks" | "config"))
    {
        return true;
    }
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        if name == ".env" || name.starts_with(".env.") {
            return true;
        }
        if matches!(
            name,
            ".bashrc" | ".zshrc" | ".zprofile" | ".profile" | ".bash_profile"
        ) {
            return true;
        }
    }
    let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return false;
    };
    [".ssh", ".gnupg", ".summonr"]
        .into_iter()
        .any(|dir| path.starts_with(home.join(dir)))
}

pub(crate) fn protected_read(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && (name == ".env"
            || name.starts_with(".env.")
            || (name.starts_with("credentials.") && crate::secret::is_store(path)))
    {
        return true;
    }
    let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return false;
    };
    [".ssh", ".gnupg"]
        .into_iter()
        .any(|dir| path.starts_with(home.join(dir)))
}

mod blocks;
mod paths;
mod shell;
mod verbs;

use blocks::*;
use paths::*;
use shell::*;

pub(crate) use paths::{expand_path, split_segments};
pub(crate) use verbs::*;

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
