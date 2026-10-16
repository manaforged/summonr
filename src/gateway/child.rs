use std::collections::VecDeque;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::oneshot;

use super::store;
use super::telegram::Bot;
use super::turn::Options;
use crate::Event;
use crate::error::{Error, Result};
use crate::tools::command::ProcessGroup;

const TYPING_EVERY: Duration = Duration::from_secs(4);
const NO_REPLY: &str = "(no reply)";
const STOPPED: &str = "stopped";
const CHECK_FAILED: i32 = 2;
const STOPPED_SHORT: i32 = 3;
const TAIL: usize = 20;

#[derive(Default)]
pub(crate) struct Seen {
    pub session: Option<String>,
    pub text: Option<String>,
    pub aborted: Option<String>,
}

impl Seen {
    pub(crate) fn read(&mut self, line: &str) {
        match serde_json::from_str::<Event>(line) {
            Ok(Event::Session { id }) => self.session = Some(id),
            Ok(Event::Assistant { text }) if !text.trim().is_empty() => self.text = Some(text),
            Ok(Event::Aborted { reason, .. }) => self.aborted = Some(reason),
            Ok(_) | Err(_) => {}
        }
    }
}

pub(crate) fn outcome(seen: &Seen, code: Option<i32>, stderr: &[String]) -> String {
    let mut parts = vec![seen.text.clone().unwrap_or_else(|| NO_REPLY.to_string())];
    parts.extend(
        seen.aborted
            .as_ref()
            .map(|reason| format!("aborted: {reason}")),
    );
    parts.extend(match code {
        Some(0) => None,
        Some(CHECK_FAILED) => Some("the check failed".to_string()),
        Some(STOPPED_SHORT) => Some("stopped short".to_string()),
        Some(code) => Some(format!("failed (exit {code})")),
        None => Some("failed (killed by a signal)".to_string()),
    });
    if !stderr.is_empty() {
        parts.push(stderr.join("\n"));
    }
    parts.join("\n\n")
}

pub(crate) fn argv(options: &Options, session: Option<&str>, text: &str) -> Vec<String> {
    let mut args: Vec<String> = options.endpoint.iter().cloned().collect();
    if let Some(model) = &options.model {
        args.extend(["--model".to_string(), model.clone()]);
    }
    if let Some(session) = session {
        args.extend(["--session".to_string(), session.to_string()]);
    }
    args.extend(["--json", "-p", "--", text].map(str::to_string));
    args
}

async fn tail(stderr: Option<tokio::process::ChildStderr>) -> Vec<String> {
    let Some(stderr) = stderr else {
        return Vec::new();
    };
    let mut lines = BufReader::new(stderr).lines();
    let mut kept = VecDeque::with_capacity(TAIL);
    while let Ok(Some(line)) = lines.next_line().await {
        if kept.len() == TAIL {
            kept.pop_front();
        }
        kept.push_back(line);
    }
    kept.into()
}

async fn halt(child: &mut tokio::process::Child, group: Option<ProcessGroup>) -> Result<()> {
    if group.as_ref().is_some_and(ProcessGroup::terminate) {
        let grace = Duration::from_secs(crate::limits::limits().gateway_stop_secs);
        if crate::supervise::exited_within(child, grace).await {
            return Ok(());
        }
    }
    drop(group);
    child.kill().await?;
    Ok(())
}

fn spawn(options: &Options, session: Option<&str>, text: &str) -> Result<tokio::process::Child> {
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .args(argv(options, session, text))
        .current_dir(&options.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    Ok(command.spawn()?)
}

pub(crate) async fn turn(
    bot: &Bot,
    options: &Options,
    chat: i64,
    text: &str,
    mut stopped: oneshot::Receiver<()>,
) -> Result<(Option<String>, String)> {
    let session = store::load()?.session(chat).map(str::to_string);
    let mut child = spawn(options, session.as_deref(), text)?;
    let mut group = child.id().map(ProcessGroup::new);
    let errors = tokio::spawn(tail(child.stderr.take()));
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Tool("the turn has no output".to_string()))?;
    let mut lines = BufReader::new(stdout).lines();
    let mut beat = tokio::time::interval(TYPING_EVERY);
    let mut seen = Seen::default();
    let mut printed = false;
    loop {
        tokio::select! {
            Ok(()) = &mut stopped => {
                halt(&mut child, group.take()).await?;
                return Ok((seen.session, STOPPED.to_string()));
            }
            _ = beat.tick() => drop(bot.typing(chat).await),
            line = lines.next_line() => match line? {
                Some(line) => {
                    printed = true;
                    seen.read(&line);
                }
                None => break,
            },
        }
    }
    let code = child.wait().await?.code();
    let stderr = errors.await.unwrap_or_default();
    let stderr = if printed { &[][..] } else { stderr.as_slice() };
    let reply = outcome(&seen, code, stderr);
    Ok((seen.session, reply))
}
