use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use super::AgentClient;
use crate::process::{self, Cap, Spec, Tail};
use summonr_core::error::{Error, ProviderFailureKind, Result};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    logged_in: bool,
    auth_method: Option<Method>,
}

#[derive(Deserialize, PartialEq, Eq)]
enum Method {
    #[serde(rename = "claude.ai")]
    Subscription,
    #[serde(other)]
    Other,
}

impl AgentClient {
    fn auth_command(&self, args: &[&str]) -> Spec {
        let mut spec = Spec::new(&self.program, args.iter().copied()).envs(self.env.clone());
        spec.remove = self.unset.iter().map(Into::into).collect();
        spec
    }

    pub(in crate::provider) async fn status(&self) -> Result<()> {
        let mut spec = self.auth_command(&["auth", "status", "--json"]);
        spec.stderr = process::Io::Null;
        let (exit, output) = tokio::time::timeout(crate::limits::limits().probe(), async {
            let mut process = process::start(spec)?;
            let mut stdout = Tail::spawn(
                Some(process.stdout),
                Cap::Head(crate::limits::limits().output_line_bytes),
            );
            let exit = process.child.wait().await?;
            stdout
                .settle()
                .await
                .map_err(|error| Error::Tool(error.to_string()))?;
            Ok::<_, Error>((exit, stdout.text()))
        })
        .await
        .map_err(|error| {
            Error::provider(
                ProviderFailureKind::Transport,
                format!("Claude Code did not return its sign-in status: {error}"),
            )
        })??;
        match serde_json::from_str::<Status>(&output) {
            Ok(status)
                if exit.success()
                    && status.logged_in
                    && status.auth_method == Some(Method::Subscription) =>
            {
                Ok(())
            }
            Err(error) if exit.success() => Err(Error::Config(format!(
                "Claude Code returned an invalid sign-in status: {error}"
            ))),
            Ok(_) | Err(_) => Err(Error::provider(
                ProviderFailureKind::MissingKey,
                "Sign in to your Claude subscription through Claude Code",
            )),
        }
    }

    pub(in crate::provider) async fn login(&self, prompt: &mut summonr_auth::Prompt) -> Result<()> {
        let mut process = process::start(self.auth_command(&["auth", "login", "--claudeai"]))
            .map_err(|error| {
                Error::Config(format!(
                    "{error}; install Claude Code to use your Claude plan"
                ))
            })?;
        let _stderr = Tail::spawn(process.stderr, Cap::Head(0));
        prompt.notice("Complete sign-in in the browser opened by Claude Code.");
        let mut stdout = BufReader::new(process.stdout);
        let mut output = Vec::new();
        let limit = crate::limits::limits().output_line_bytes;
        (&mut stdout)
            .take(limit as u64)
            .read_until(b'>', &mut output)
            .await?;
        if output.len() == limit {
            return Err(Error::Config(
                "Claude Code sign-in output exceeded the limit".into(),
            ));
        }
        let text = String::from_utf8_lossy(&output);
        let url = text
            .split_whitespace()
            .find(|word| word.starts_with("https://"));
        if let Some(url) = url {
            prompt.notice(&format!("open {url}"));
        }
        let _stdout = Tail::spawn(Some(stdout), Cap::Head(0));
        let status = if output.last() == Some(&b'>') {
            tokio::select! {
                status = process.child.wait() => status?,
                code = prompt.request("Paste the code if the browser shows one") => {
                    let code = code.map_err(|error| Error::Config(error.to_string()))?;
                    process.stdin.write_all(code.as_bytes()).await?;
                    process.stdin.write_all(b"\n").await?;
                    process.stdin.flush().await?;
                    process.child.wait().await?
                }
            }
        } else {
            process.child.wait().await?
        };
        if !status.success() {
            return Err(Error::Config(format!(
                "Claude Code sign-in failed ({status})"
            )));
        }
        self.status().await
    }
}
