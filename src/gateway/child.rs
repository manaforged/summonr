use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::oneshot;

use super::store;
use super::telegram::Bot;
use super::turn::Options;
use crate::cron::launch::Launch;
use crate::cron::report::Report;
use crate::cron::store::{Outcome, Task};
use crate::error::{Error, Result};
use crate::process::{Cap, Child, Io, Spec, Tail};
use crate::tools::command::ProcessGroup;

const TYPING_EVERY: Duration = Duration::from_secs(4);
const NO_REPLY: &str = "(no reply)";
const STOPPED: &str = "stopped";

pub(crate) fn outcome(seen: &Report, code: Option<i32>, stderr: &[String]) -> String {
    let mut parts = vec![seen.text.clone().unwrap_or_else(|| NO_REPLY.to_string())];
    parts.extend(
        seen.aborted
            .as_ref()
            .map(|reason| format!("aborted: {reason}")),
    );
    parts.extend(match (Outcome::from_code(code), code) {
        (Outcome::Done, _) => None,
        (Outcome::CheckFailed, _) => Some("the check failed".to_string()),
        (Outcome::Unfinished, _) => Some("stopped short".to_string()),
        (_, Some(code)) => Some(format!("failed (exit {code})")),
        (_, None) => Some("failed (killed by a signal)".to_string()),
    });
    if !stderr.is_empty() {
        parts.push(stderr.join("\n"));
    }
    parts.join("\n\n")
}

pub(crate) fn argv(options: &Options, session: Option<&str>, text: &str) -> Vec<String> {
    Launch {
        endpoint: options.endpoint.as_deref(),
        model: options.model.as_deref(),
        session,
        max_secs: None,
        max_tokens: None,
        worktree: false,
        task: &Task::Prompt(text.to_string()),
    }
    .argv()
}

async fn halt(child: &mut Child, group: Option<ProcessGroup>) -> Result<()> {
    if group.as_ref().is_some_and(ProcessGroup::terminate) {
        let grace = Duration::from_secs(crate::limits::limits().gateway_stop_secs);
        if crate::supervise::within(grace, child.wait()).await {
            return Ok(());
        }
    }
    drop(group);
    child.kill().await?;
    Ok(())
}

fn spawn(options: &Options, session: Option<&str>, text: &str) -> Result<Child> {
    crate::process::spawn(Spec {
        cwd: Some(options.dir.clone()),
        stdin: Io::Null,
        ..Spec::new(std::env::current_exe()?, argv(options, session, text))
    })
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
    let (_, stdout, stderr) = crate::process::pipes(&mut child);
    let tail = crate::limits::limits().gateway_stderr_tail_lines;
    let mut errors = Tail::spawn(stderr, Cap::Lines(tail));
    let stdout = stdout.ok_or_else(|| Error::Tool("the turn has no output".to_string()))?;
    let mut lines = BufReader::new(stdout).lines();
    let mut beat = tokio::time::interval(TYPING_EVERY);
    let mut seen = Report::default();
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
    drop(errors.settle().await);
    let stderr = errors.lines(tail);
    let stderr = if printed { &[][..] } else { stderr.as_slice() };
    let reply = outcome(&seen, code, stderr);
    Ok((seen.session, reply))
}
