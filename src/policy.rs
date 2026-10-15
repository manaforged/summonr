use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

mod ask;
mod danger;
mod protected;
pub(crate) use protected::{holds_protected, protected_read, workspace_control};
mod walk;

pub(crate) use walk::{normalize, shell_words};
pub(crate) use {
    danger::danger,
    walk::{argv, verb},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Policy {
    pub strict: bool,
    confirm: Gate,
    session_confirm: Gate,
    read_only: bool,
    unrestricted: bool,
    deny: Vec<String>,
    ask: Vec<String>,
    allow: Vec<Vec<String>>,
    write: Vec<Vec<String>>,
    roots: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Grant {
    Write(PathBuf),
    Guarded(PathBuf),
    Command(String),
    Danger { command: String, reason: String },
    Network,
}

impl Grant {
    #[must_use]
    pub(crate) fn prompt(&self) -> String {
        match self {
            Self::Write(path) => path.display().to_string(),
            Self::Guarded(path) => format!("protected path {}", path.display()),
            Self::Command(command) => format!("a shell command without the sandbox: {command}"),
            Self::Danger { command, reason } => {
                format!(
                    "{command} — {}",
                    reason.strip_prefix("it ").unwrap_or(reason)
                )
            }
            Self::Network => "network".to_string(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct File {
    #[serde(default)]
    strict: bool,
    #[serde(default)]
    confirm: Gate,
    #[serde(default)]
    deny: Vec<String>,
    #[serde(default)]
    ask: Vec<String>,
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
        self.confirm = self.confirm.max(file.confirm);
        ask::extend_unique(&mut self.deny, file.deny);
        ask::extend_unique(&mut self.ask, file.ask);
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
    pub(crate) fn is_strict(&self) -> bool {
        self.strict
    }

    #[must_use]
    pub(crate) fn configured(&self) -> bool {
        self.strict || !self.deny.is_empty() || !self.allow.is_empty() || !self.write.is_empty()
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

    pub(crate) fn command(&self, command: &str, workspace: &Path, start: &Path) -> Result<()> {
        let command = command.trim();
        let segments = split_segments(command);
        self.command_policy_checked(command, &segments, workspace, start)?;
        if self.unrestricted {
            return Ok(());
        }
        let steps = walk::walk(command, workspace, start, &self.roots);
        match write_block(&steps, workspace, &self.roots) {
            Some(Block::Protected(path)) => {
                return Err(Error::Denied(format!("`{path}` is a protected path")));
            }
            Some(Block::Outside(path)) => {
                return Err(Error::Denied(format!(
                    "`{path}` is outside the workspace: {}",
                    outside_hint(&path)
                )));
            }
            None => {}
        }
        if let Some(path) = read_block(&steps, workspace) {
            return Err(Error::Denied(format!("`{path}` is a protected path")));
        }
        Ok(())
    }

    pub(crate) fn command_policy_checked(
        &self,
        command: &str,
        segments: &[String],
        workspace: &Path,
        start: &Path,
    ) -> Result<()> {
        if command.is_empty() {
            return Err(Error::Denied("command is empty".to_string()));
        }
        if self.unrestricted {
            return Ok(());
        }
        if self.strict
            && (builtin_deny(command, segments)
                || danger(command, workspace, start, Gate::Risky).is_some())
        {
            return Err(Error::Denied(format!(
                "blocked command `{command}`; strict mode never runs it; run it yourself in another terminal"
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

    pub(crate) fn tool(&self, alias: &str, readonly: bool) -> Result<()> {
        if self.read_only && !readonly {
            return Err(Error::Denied(format!(
                "`{alias}` can change state and is unavailable in a read-only session"
            )));
        }
        if self.unrestricted {
            return Ok(());
        }
        if let Some(pattern) = self
            .deny
            .iter()
            .find(|pattern| tool_matches(alias, pattern))
        {
            return Err(Error::Denied(format!("denied by policy `{pattern}`")));
        }
        if readonly
            || !self.strict
            || !self.allow.is_empty()
                && self
                    .allow
                    .iter()
                    .all(|list| list.iter().any(|pattern| tool_matches(alias, pattern)))
        {
            return Ok(());
        }
        Err(Error::Denied(format!(
            "`{alias}` can change state and is not on every configured allow list"
        )))
    }

    pub(crate) fn write_path(&self, relative: &str) -> Result<()> {
        if self.read_only {
            return Err(Error::Denied(
                "this session is read-only and does not edit files. \
                 Report the change in your reply."
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
    if git::guarded_git_part(&parts) || protected::workspace_control(path) {
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
    protected::home_secret(path, &[".ssh", ".gnupg", ".summonr"])
}

mod blocks;
pub(crate) mod git;
mod paths;
mod shell;
mod verbs;

use blocks::*;
pub(crate) use blocks::{
    grant_from_error, implicit_write, outside_hint, outside_write, reads_only, write_denied,
};
use paths::*;
use shell::*;

#[cfg(any(target_os = "linux", windows, test))]
pub(crate) use git::guarded_git_paths;
#[cfg(any(target_os = "macos", test))]
pub(crate) use git::guarded_git_regex;
pub(crate) use paths::{expand_path, split_segments, unquoted};
#[cfg(all(test, unix))]
pub(crate) use shell::running_as_root;
pub(crate) use verbs::*;

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;

fn tool_matches(alias: &str, pattern: &str) -> bool {
    pattern
        .trim()
        .strip_suffix('*')
        .map_or(alias == pattern.trim(), |prefix| alias.starts_with(prefix))
}
