use summonr_core::error::{Error, ErrorKind, Result};

use super::{Builtin, ToolRegistry};
use crate::provider::UserInput;
use crate::skills::{Body, Piece, Scope};

const DISABLED: &str = "[shell command execution disabled by policy]";

impl ToolRegistry {
    /// Lets skills and commands defined in the workspace run their `!` blocks; user and plugin files always may.
    pub(crate) fn set_trust(&mut self, trust: bool) {
        self.trust = trust;
    }

    /// Adds the deny and ask rules and the environment that imported Claude Code settings give.
    pub(crate) fn import_claude(&mut self, settings: crate::config::claude::Settings) {
        self.policy.narrow(settings.deny, settings.ask);
        self.imported = settings.env;
    }

    /// The environment imported Claude Code settings give tool commands and hooks.
    pub(crate) fn imported_env(&self) -> &[(String, String)] {
        &self.imported
    }

    /// The variables a tool command gets: imported settings first, so the endpoint's own win.
    pub(crate) fn command_vars(&self) -> Vec<(String, String)> {
        self.imported.iter().chain(&self.env).cloned().collect()
    }

    /// `body` with each `@path` file attached through the read policy and each `!` block replaced by its command's output; any refusal or failure stops the whole invocation.
    pub(crate) async fn inject(&self, body: &Body) -> Result<UserInput> {
        let written: Vec<&str> = body
            .pieces
            .iter()
            .filter_map(|piece| match piece {
                Piece::Text(text) => Some(text.as_str()),
                Piece::Shell { .. } => None,
            })
            .collect();
        let (files, images) =
            super::workspace::attachments(&self.workspace, &written.join(" "), |path| {
                self.resolve_read(path, super::paths::Shield::Contained)
            })?;
        let runs = body.scope == Scope::User || self.trust;
        let mut text = String::new();
        for piece in &body.pieces {
            match piece {
                Piece::Text(kept) => text.push_str(kept),
                Piece::Shell { .. } if !runs => text.push_str(DISABLED),
                Piece::Shell { pattern, command } => {
                    let output = self.injected(pattern, command).await?;
                    text.push_str(&crate::secret::shield::scrub_found(
                        &output,
                        &self.workspace,
                    ));
                }
            }
        }
        let files = crate::secret::shield::scrub_found(&files, &self.workspace);
        Ok(UserInput {
            text: super::workspace::joined(&text, &files),
            images,
            source: None,
        })
    }

    async fn injected(&self, pattern: &str, command: &str) -> Result<String> {
        let refused = |reason: &str| {
            Error::Denied(format!(
                "Shell command permission check failed for pattern \"{pattern}\": {reason}"
            ))
        };
        let failed = |output: &str| {
            Error::Tool(format!(
                "Shell command failed for pattern \"{pattern}\": {output}"
            ))
        };
        let args = serde_json::json!({ "command": command });
        let ran = match self.available(Builtin::RunCommand) {
            Ok(()) => self.fit_output(self.run_command(&args).await),
            Err(error) => Err(error),
        };
        let output = match ran {
            Ok(output) => output,
            Err(error) if error.kind() == ErrorKind::Denied => {
                return Err(refused(&error.detail()));
            }
            Err(error) => return Err(failed(&error.detail())),
        };
        if output.grant.is_some() {
            return Err(refused(&output.text));
        }
        super::command::passed(command, &output).ok_or_else(|| failed(&output.text))
    }
}
