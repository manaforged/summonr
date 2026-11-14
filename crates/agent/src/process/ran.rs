use std::time::Duration;

use tokio::io::AsyncWriteExt as _;

use super::{Cap, Process, Tail};
use summonr_core::error::Result;

/// The exit code and output of a finished process.
pub(crate) struct Ran {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl Ran {
    /// The same run with surrounding whitespace trimmed from both streams.
    pub(crate) fn trimmed(self) -> Self {
        Self {
            code: self.code,
            stdout: self.stdout.trim().to_string(),
            stderr: self.stderr.trim().to_string(),
        }
    }

    /// Why the process failed: its trimmed stderr, or its exit code when stderr is blank.
    pub(crate) fn reason(&self) -> String {
        match self.stderr.trim() {
            "" => format!("exited with {:?}", self.code),
            reason => reason.to_string(),
        }
    }
}

/// Feeds `input` to a started process and waits up to `limit`, keeping each stream within its cap.
pub(crate) async fn finish(
    process: Process,
    input: &[u8],
    keep: (Cap, Cap),
    limit: Duration,
    what: impl Into<String>,
) -> Result<std::io::Result<Ran>> {
    let Process {
        mut child,
        mut stdin,
        stdout,
        stderr,
        group,
    } = process;
    let mut stdout = Tail::spawn(Some(stdout), keep.0);
    let mut stderr = Tail::spawn(stderr, keep.1);
    let work = async move {
        drop(stdin.write_all(input).await);
        drop(stdin);
        let status = child.wait().await?;
        drop(stdout.settle().await);
        drop(stderr.settle().await);
        Ok::<_, std::io::Error>(Ran {
            code: status.code(),
            stdout: stdout.text(),
            stderr: stderr.text(),
        })
    };
    let finished = crate::limits::within(limit, what, work).await;
    drop(group);
    finished
}
