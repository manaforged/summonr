use std::future::Future;
use std::pin::Pin;

use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::{ChildStdin, ChildStdout};
use tokio::time::Instant;

use super::super::frame::{Framing, Lines, Read, encode};
use crate::process::{Child, Tail};
use crate::tools::command::ProcessGroup;
use summonr_core::error::{Error, Result};

pub(crate) type Pending<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait HttpTransport: Send + Sync {
    fn send<'a>(&'a mut self, value: &'a Value) -> Pending<'a, Result<Option<Value>>>;
    fn next(&mut self) -> Pending<'_, std::io::Result<Read>>;
    fn expired(&self) -> bool;
    fn agree(&mut self, version: &str);
}

pub(crate) enum Transport {
    Stdio(Box<Stdio>),
    Http(Box<dyn HttpTransport>),
}

pub(crate) struct Stdio {
    pub(super) group: Option<ProcessGroup>,
    pub(super) child: Option<Child>,
    pub(super) stdin: Option<ChildStdin>,
    pub(super) reader: Lines<ChildStdout>,
    pub(super) framing: Framing,
    pub(super) stderr: Tail,
    pub(super) secrets: Vec<String>,
}

pub(super) enum Sent {
    Delivered,
    Refused(Value),
    Late,
}

impl Transport {
    pub(super) async fn send(&mut self, value: &Value, deadline: Instant) -> Result<Sent> {
        let sent = match self {
            Self::Stdio(stdio) => {
                let bytes = encode(stdio.framing, value)?;
                tokio::time::timeout_at(deadline, stdio.write(&bytes))
                    .await
                    .map(|written| written.map(|()| None).map_err(Error::from))
            }
            Self::Http(http) => tokio::time::timeout_at(deadline, http.send(value)).await,
        };
        match sent {
            Ok(Ok(None)) => Ok(Sent::Delivered),
            Ok(Ok(Some(error))) => Ok(Sent::Refused(error)),
            Ok(Err(error)) => Err(error),
            Err(_elapsed) => Ok(Sent::Late),
        }
    }

    pub(super) async fn next(&mut self) -> std::io::Result<Read> {
        match self {
            Self::Stdio(stdio) => stdio.reader.next().await,
            Self::Http(http) => http.next().await,
        }
    }

    pub(super) fn piped(&self) -> bool {
        matches!(self, Self::Stdio(_))
    }
}

impl Stdio {
    pub(super) async fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let pipe = self
            .stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
        pipe.write_all(bytes).await?;
        pipe.flush().await
    }
}

impl Stdio {
    pub(super) fn note(&self) -> String {
        self.stderr
            .last_line(&self.secrets)
            .map(|line| format!("; stderr: {line}"))
            .unwrap_or_default()
    }
}

impl Drop for Stdio {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let (Some(child), group) = (self.child.take(), self.group.take()) else {
            return;
        };
        crate::supervise::close(&runtime, child, group);
    }
}
