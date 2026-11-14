use std::path::Path;

use super::*;

pub(super) type Check = std::result::Result<(), String>;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Action<'a> {
    Command {
        text: &'a str,
        workspace: &'a Path,
        cwd: &'a Path,
        ask: bool,
        approved: bool,
    },
    Read {
        path: &'a Path,
        shielded: bool,
    },
    Write(&'a str),
    Tool {
        alias: &'a str,
        read_only: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Allow,
    Ask(String),
    Deny(String),
}

impl Decision {
    pub(super) fn from_check(check: Check) -> Self {
        check.map_or_else(Self::Deny, |()| Self::Allow)
    }

    pub(crate) fn into_result(self) -> Result<()> {
        match self {
            Self::Ask(reason) | Self::Deny(reason) => Err(Error::Denied(reason)),
            Self::Allow => Ok(()),
        }
    }
}

impl Policy {
    #[must_use]
    pub(crate) fn decide(&self, action: Action<'_>) -> Decision {
        match action {
            Action::Command {
                text,
                workspace,
                cwd,
                ask,
                approved,
            } => {
                if ask
                    && !self.strict
                    && let Some(reason) = self.needs_approval(text, workspace, cwd)
                {
                    return Decision::Ask(reason);
                }
                let text = text.trim();
                Decision::from_check(if approved {
                    self.command_rules(text, &split_segments(text), workspace, cwd)
                } else {
                    self.command_paths(text, workspace, cwd)
                })
            }
            Action::Read { path, shielded } => {
                if (shielded || !self.unrestricted) && protected_read(path) {
                    Decision::Deny(format!("`{}` is a protected path", path.display()))
                } else {
                    Decision::Allow
                }
            }
            Action::Write(relative) => Decision::from_check(self.write_rules(relative)),
            Action::Tool { alias, read_only } => {
                Decision::from_check(self.tool_rules(alias, read_only))
            }
        }
    }
}
