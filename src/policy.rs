use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

mod ask;
mod danger;
mod decide;
mod protected;
pub(crate) use protected::{holds_protected, protected_read, workspace_control};
mod walk;

use decide::Check;
pub(crate) use decide::{Action, Decision};
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
    Danger { command: String, reason: String },
}

impl Grant {
    #[must_use]
    pub(crate) fn prompt(&self) -> String {
        match self {
            Self::Write(path) => path.display().to_string(),
            Self::Guarded(path) => format!("protected path {}", path.display()),
            Self::Danger { command, reason } => {
                format!(
                    "{command} — {}",
                    reason.strip_prefix("it ").unwrap_or(reason)
                )
            }
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
        policy.roots = crate::config::file()?.write_root_paths();
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
        self.decide(Action::Command {
            text: command,
            workspace,
            cwd: start,
            ask: false,
            approved: false,
        })
        .into_result()
    }

    #[cfg(test)]
    pub(crate) fn command_policy_checked(
        &self,
        command: &str,
        segments: &[String],
        workspace: &Path,
        start: &Path,
    ) -> Result<()> {
        self.command_rules(command, segments, workspace, start)
            .map_err(Error::Denied)
    }

    pub(crate) fn write_path(&self, relative: &str) -> Result<()> {
        self.decide(Action::Write(relative)).into_result()
    }

    #[cfg(test)]
    pub(crate) fn write_globs(&self, relative: &str) -> Result<()> {
        self.glob_rules(relative).map_err(Error::Denied)
    }

    fn command_paths(&self, command: &str, workspace: &Path, start: &Path) -> Check {
        let segments = split_segments(command);
        self.command_rules(command, &segments, workspace, start)?;
        if self.unrestricted {
            return Ok(());
        }
        let steps = walk::walk(command, workspace, start, &self.roots);
        match write_block(&steps, workspace, &self.roots) {
            Some(Block::Protected(path)) => {
                return Err(format!("`{path}` is a protected path"));
            }
            Some(Block::Outside(path)) => {
                return Err(format!(
                    "`{path}` is outside the workspace: {}",
                    outside_hint(&path)
                ));
            }
            None => {}
        }
        if let Some(path) = read_block(&steps, workspace) {
            return Err(format!("`{path}` is a protected path"));
        }
        Ok(())
    }

    fn command_rules(
        &self,
        command: &str,
        segments: &[String],
        workspace: &Path,
        start: &Path,
    ) -> Check {
        if command.is_empty() {
            return Err("command is empty".to_string());
        }
        if self.unrestricted {
            return Ok(());
        }
        if self.strict
            && (builtin_deny(command, segments)
                || danger(command, workspace, start, Gate::Risky).is_some())
        {
            return Err(format!(
                "blocked command `{command}`; strict mode never runs it; run it yourself in another terminal"
            ));
        }
        for pattern in &self.deny {
            if matches_deny(command, pattern) {
                return Err(format!("denied by policy `{pattern}`"));
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
            return Err(format!(
                "command `{command}` is not on every configured allow list"
            ));
        }
        Ok(())
    }

    fn tool_rules(&self, alias: &str, readonly: bool) -> Check {
        if self.read_only && !readonly {
            return Err(format!(
                "`{alias}` can change state and is unavailable in a read-only session"
            ));
        }
        if self.unrestricted {
            return Ok(());
        }
        if let Some(pattern) = self
            .deny
            .iter()
            .find(|pattern| tool_matches(alias, pattern))
        {
            return Err(format!("denied by policy `{pattern}`"));
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
        Err(format!(
            "`{alias}` can change state and is not on every configured allow list"
        ))
    }

    fn write_rules(&self, relative: &str) -> Check {
        if self.read_only {
            return Err("this session is read-only and does not edit files. \
                 Report the change in your reply."
                .to_string());
        }
        if !self.unrestricted && protected_write(Path::new(relative)) {
            return Err(format!("`{relative}` is a protected path"));
        }
        self.glob_rules(relative)
    }

    fn glob_rules(&self, relative: &str) -> Check {
        if self.unrestricted || self.write.is_empty() {
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
        Err(format!(
            "write to `{relative}` is not permitted by the policy write globs"
        ))
    }
}

pub(crate) fn protected_write(path: &Path) -> bool {
    let unix = path.to_string_lossy().replace('\\', "/");
    let parts: Vec<&str> = unix.split('/').filter(|part| !part.is_empty()).collect();
    if git::guarded_git_part(&parts) || protected::workspace_control(path) {
        return true;
    }
    let secrets = &rules().secrets;
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && protected::secret_name(name, &secrets.write_files, &secrets.write_prefixes)
    {
        return true;
    }
    protected::home_secret(path, secrets.write_dirs.as_slice())
}

mod blocks;
pub(crate) mod git;
mod paths;
mod shell;
mod verbs;

use blocks::*;
pub(crate) use blocks::{grant_from_error, outside_hint, reads_only, write_denied};
use paths::*;
use shell::*;

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
