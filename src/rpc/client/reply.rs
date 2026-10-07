use serde_json::{Value, json};
use tokio::time::Instant;

use super::{Client, Read};
use crate::error::{Error, Result};
use crate::limits::{MCP_FRAME_MAX, MCP_PROGRESS_STRETCH, MCP_STDERR_SETTLE};
use crate::rpc::{Ask, Frame, Notice, Reply, ask, failure, notice, outcome, parse, response};

struct Clock {
    deadline: Instant,
    hard: Instant,
}

impl Client {
    pub(super) async fn reply(
        &mut self,
        id: &Value,
        method: &str,
        deadline: Instant,
        on_note: &mut impl FnMut(Notice<'_>),
    ) -> Result<Value> {
        let mut clock = Clock {
            deadline,
            hard: Instant::now() + self.wait.saturating_mul(MCP_PROGRESS_STRETCH),
        };
        loop {
            let read = match tokio::time::timeout_at(clock.deadline, self.frame()).await {
                Ok(read) => read?,
                Err(_elapsed) if self.broken => {
                    if let Some(drain) = &self.drain {
                        drain.abort();
                    }
                    self.settle_stderr().await?;
                    Read::Closed
                }
                Err(_elapsed) => return Err(self.give_up(id, method).await),
            };
            if matches!(read, Read::Closed) && self.drain.is_some() {
                clock.deadline = clock.deadline.min(Instant::now() + MCP_STDERR_SETTLE);
                continue;
            }
            let Some(line) = self.line(read, method)? else {
                continue;
            };
            if line.trim().is_empty() {
                continue;
            }
            let parsed = parse(&line);
            drop(line);
            let Ok(frame) = parsed else {
                continue;
            };
            match frame {
                Frame::Response {
                    id: got,
                    result,
                    error,
                } if got == *id => return outcome(method, result, error),
                Frame::Response { id: got, .. } => self.abandoned.retain(|old| *old != got),
                Frame::Notification { method, params } => {
                    let notice = notice(&method, &params);
                    if matches!(notice, Notice::Progress { token, .. } if token == id) {
                        clock.deadline = (Instant::now() + self.wait).min(clock.hard);
                    }
                    on_note(notice);
                }
                Frame::Request {
                    id: asked,
                    method,
                    params,
                } => {
                    let answer = self
                        .answer(&asked, &method, &params, &mut clock, on_note)
                        .await;
                    self.write(&answer).await?;
                }
            }
        }
    }

    fn line(&mut self, read: Read, method: &str) -> Result<Option<String>> {
        match read {
            Read::Line(line) => Ok(Some(line)),
            Read::Oversize if !self.abandoned.is_empty() => {
                self.abandoned.remove(0);
                Ok(None)
            }
            Read::Oversize => Err(Error::Tool(format!(
                "a reply to `{method}` was over {} MiB; summonr dropped it",
                MCP_FRAME_MAX / (1024 * 1024)
            ))),
            Read::Closed => Err(Error::Tool(format!(
                "the child exited during `{method}`{}",
                self.stderr_note()
            ))),
        }
    }

    async fn answer(
        &self,
        asked: &Value,
        method: &str,
        params: &Value,
        clock: &mut Clock,
        on_note: &mut impl FnMut(Notice<'_>),
    ) -> Value {
        match ask(method) {
            Ask::Roots => response(asked, json!({"roots": self.roots})),
            Ask::Ping => response(asked, json!({})),
            Ask::Elicit => {
                let (sender, answered) = tokio::sync::oneshot::channel();
                on_note(Notice::Elicit {
                    params,
                    answer: sender,
                });
                let reply = answered
                    .await
                    .unwrap_or_else(|_| Reply::Result(json!({"action": "cancel"})));
                clock.deadline = Instant::now() + self.wait;
                clock.hard = clock.hard.max(clock.deadline);
                match reply {
                    Reply::Result(result) => response(asked, result),
                    Reply::Error(code, message) => failure(asked, code, &message),
                }
            }
            Ask::Unsupported => failure(asked, -32601, &format!("{method} is not supported")),
        }
    }
}
