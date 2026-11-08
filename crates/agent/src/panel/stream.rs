use std::path::Path;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::{Content, Panel};
use crate::process::{Cap, Process, Tail};

/// A stream panel's running command; dropping it stops the command and its process group.
pub(crate) struct Stream {
    input: UnboundedSender<String>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Stream {
    /// Starts the command; `shown` gets one surface per printed line, then the reason it stopped.
    pub(crate) fn start(
        panel: &Panel,
        workspace: &Path,
        shown: impl Fn(Result<Content, String>) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let process = super::run::start(panel, workspace)?;
        let (input, inbox) = unbounded_channel();
        let task = tokio::spawn(async move {
            let reason = pump(process, inbox, &shown).await;
            shown(Err(reason));
        });
        Ok(Self { input, task })
    }

    /// Whether the command still runs.
    pub(crate) fn live(&self) -> bool {
        !self.task.is_finished()
    }

    /// Sends one line, such as an A2UI action message, to the command's stdin.
    pub(crate) fn send(&self, line: String) -> bool {
        self.input.send(line).is_ok()
    }
}

async fn pump(
    process: Process,
    mut inbox: UnboundedReceiver<String>,
    shown: &(impl Fn(Result<Content, String>) + Sync),
) -> String {
    let limits = crate::limits::limits();
    let max = limits.panel_output_bytes;
    let (mut child, _group, mut stdin) = (process.child, process.group, process.stdin);
    let mut stderr = Tail::spawn(process.stderr, Cap::Tail(limits.mcp_stderr_tail_bytes));
    let mut stdout = BufReader::new(process.stdout);
    let mut line = Vec::new();
    let mut open = true;
    loop {
        let mut limited = (&mut stdout).take(max as u64 + 1);
        tokio::select! {
            read = limited.read_until(b'\n', &mut line) => match read {
                Ok(0) => break,
                Ok(_) => {
                    if let Some(reason) = take_line(&mut line, max, shown) {
                        return reason;
                    }
                }
                Err(error) => return format!("output failed: {error}"),
            },
            next = inbox.recv(), if open => match next {
                Some(text) => drop(stdin.write_all(format!("{text}\n").as_bytes()).await),
                None => open = false,
            },
        }
    }
    let status = child.wait().await;
    drop(stderr.settle().await);
    match (status, stderr.text().trim()) {
        (Ok(status), "") => format!("stopped: exited with {:?}", status.code()),
        (Ok(_), reason) => reason.to_string(),
        (Err(error), _) => error.to_string(),
    }
}

fn take_line(
    line: &mut Vec<u8>,
    max: usize,
    shown: &impl Fn(Result<Content, String>),
) -> Option<String> {
    if line.len() > max {
        return Some(format!("printed a line longer than {max} bytes"));
    }
    if line.ends_with(b"\n") {
        shown(
            std::str::from_utf8(line)
                .map_err(|error| format!("printed a line that is not UTF-8: {error}"))
                .and_then(Content::parse),
        );
        line.clear();
    }
    None
}
