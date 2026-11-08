use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdout};
use std::time::{Duration, Instant};

use summonr_core::error::{Error, Result};

use crate::process::{Io, Spec, spawn_blocking};
use crate::tools::args::{object_bool, object_string};

use super::{ToolOutput, ToolRegistry};

const DEFAULT_TIMEOUT_MS: u64 = 300_000;
const MAX_TIMEOUT_MS: u64 = 3_600_000;
const BATCH: Duration = Duration::from_millis(200);

impl ToolRegistry {
    pub(in crate::tools) async fn monitor(&self, args: &serde_json::Value) -> Result<ToolOutput> {
        let description = object_string(args, "description")?;
        let command = object_string(args, "command")?;
        if description.trim().is_empty() || command.trim().is_empty() {
            return Err(Error::Tool(
                "monitor needs a description and a command".to_string(),
            ));
        }
        let session = self
            .session
            .lock()
            .ok()
            .and_then(|held| held.clone())
            .ok_or_else(|| Error::Tool("monitor needs a session".to_string()))?;
        let cwd = self.command_dir(args)?;
        if let Some(output) =
            self.gate_command(command, &cwd, self.take_approval(command, &cwd)?)?
        {
            return Ok(output);
        }
        self.check_launch(false, self.policy.restricts_writes(), true)?;
        let persistent = object_bool(args, "persistent");
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .clamp(1_000, MAX_TIMEOUT_MS);
        let spec = super::proc::shell(command, &cwd)?.envs(self.env.iter().cloned());
        let mut child = spawn_blocking(Spec {
            stdin: Io::Null,
            stderr: Io::Null,
            ..spec
        })?;
        let pid = child.id();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Tool("monitor started without stdout".to_string()))?;
        let group = super::proc::ProcessGroup::new(pid);
        let label = description.to_string();
        let timeout = if persistent {
            None
        } else {
            Some(tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
                super::proc::kill_group(pid);
            }))
        };
        std::thread::spawn(move || {
            watch(child, stdout, group, session, label, timeout);
        });
        Ok(ToolOutput::ok(format!(
            "monitor {pid}: {description}. A stdout line wakes this session. Silence does not."
        )))
    }
}

fn watch(
    mut child: Child,
    stdout: ChildStdout,
    mut group: super::proc::ProcessGroup,
    session: String,
    label: String,
    timeout: Option<tokio::task::JoinHandle<()>>,
) {
    let lines = BufReader::new(stdout).lines();
    let mut batch = String::new();
    let mut last = Instant::now();
    for line in lines {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if !batch.is_empty() {
            batch.push('\n');
        }
        batch.push_str(&line);
        if last.elapsed() >= BATCH {
            flush(&session, &label, &batch);
            batch.clear();
            last = Instant::now();
        }
    }
    if !batch.is_empty() {
        flush(&session, &label, &batch);
    }
    let outcome = child.wait();
    if let Some(timeout) = timeout {
        timeout.abort();
    }
    group.disarm();
    match outcome {
        Ok(status) if !status.success() => {
            flush(&session, &label, &format!("monitor exited: {status}"));
        }
        Ok(_) => {}
        Err(error) => flush(&session, &label, &format!("monitor wait failed: {error}")),
    }
}

fn flush(session: &str, label: &str, batch: &str) {
    let text = format!("{label}: {batch}");
    drop(crate::mail::Mailbox::wake(session, &text));
}

#[cfg(test)]
#[path = "monitor_tests.rs"]
mod tests;
