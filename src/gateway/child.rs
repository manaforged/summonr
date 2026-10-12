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
const TAIL: usize = 20;
const CHECK_FAILED: i32 = 2;
const STOPPED_SHORT: i32 = 3;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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

    pub(crate) fn reply(&self) -> String {
        let text = self.text.as_deref().unwrap_or(NO_REPLY);
        match &self.aborted {
            Some(reason) => format!("{text}\n\naborted: {reason}"),
            None => text.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exit {
    Done,
    CheckFailed,
    Short,
    Failed(i32),
    Killed,
}

impl Exit {
    pub(crate) fn from_code(code: Option<i32>) -> Self {
        match code {
            Some(0) => Self::Done,
            Some(CHECK_FAILED) => Self::CheckFailed,
            Some(STOPPED_SHORT) => Self::Short,
            Some(code) => Self::Failed(code),
            None => Self::Killed,
        }
    }

    fn note(self) -> Option<String> {
        match self {
            Self::Done => None,
            Self::CheckFailed => Some("the check failed".to_string()),
            Self::Short => Some("stopped short".to_string()),
            Self::Failed(code) => Some(format!("failed (exit {code})")),
            Self::Killed => Some("failed (killed by a signal)".to_string()),
        }
    }
}

pub(crate) fn outcome(seen: &Seen, exit: Exit, stderr: &[String]) -> String {
    let mut text = seen.reply();
    if let Some(note) = exit.note() {
        text = format!("{text}\n\n{note}");
    }
    if !stderr.is_empty() {
        text = format!("{text}\n\n{}", stderr.join("\n"));
    }
    text
}

pub(crate) fn argv(options: &Options, session: Option<&str>, text: &str) -> Vec<String> {
    let mut args: Vec<String> = options.endpoint.iter().cloned().collect();
    if let Some(model) = &options.model {
        args.extend(["--model".to_string(), model.clone()]);
    }
    if let Some(session) = session {
        args.extend(["--session".to_string(), session.to_string()]);
    }
    args.extend([
        "--json".to_string(),
        "-p".to_string(),
        "--".to_string(),
        text.to_string(),
    ]);
    args
}

pub(crate) struct Turn {
    pub session: Option<String>,
    pub reply: String,
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
) -> Result<Turn> {
    let session = store::load()?.session(chat).map(str::to_string);
    let mut child = spawn(options, session.as_deref(), text)?;
    let mut group = child.id().map(|pid| ProcessGroup::new(pid, false));
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
                return Ok(Turn { session: seen.session, reply: STOPPED.to_string() });
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
    let exit = Exit::from_code(child.wait().await?.code());
    let stderr = errors.await.unwrap_or_default();
    let stderr = if printed { &[][..] } else { stderr.as_slice() };
    Ok(Turn {
        reply: outcome(&seen, exit, stderr),
        session: seen.session,
    })
}
