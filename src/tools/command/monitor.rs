use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use summonr_core::error::{Error, Result};

use crate::tools::args::{object_bool, object_string};

use super::{ToolOutput, ToolRegistry};

const DEFAULT_TIMEOUT_MS: u64 = 300_000;
const MAX_TIMEOUT_MS: u64 = 3_600_000;
const BATCH: Duration = Duration::from_millis(200);

impl ToolRegistry {
    pub(super) fn monitor(&self, args: &serde_json::Value) -> Result<ToolOutput> {
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
        if let Some(output) = self.gate_command(command, &cwd, false)? {
            return Ok(output);
        }
        self.check_launch(false, self.policy.restricts_writes(), true)?;
        let persistent = object_bool(args, "persistent");
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .clamp(1_000, MAX_TIMEOUT_MS);
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&cwd)
            .envs(self.env.iter().cloned())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| Error::Tool(format!("monitor failed to start: {error}")))?;
        let pid = child.id();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Tool("monitor started without stdout".to_string()))?;
        let group = super::proc::ProcessGroup::new(pid);
        let label = description.to_string();
        if !persistent {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(timeout_ms));
                super::proc::kill_group(pid);
            });
        }
        std::thread::spawn(move || {
            watch(child, stdout, group, session, label);
        });
        Ok(ToolOutput::ok(format!(
            "monitor {pid}: {description}. A stdout line wakes this session. Silence does not."
        )))
    }
}

fn watch(
    mut child: std::process::Child,
    stdout: std::process::ChildStdout,
    mut group: super::proc::ProcessGroup,
    session: String,
    label: String,
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
    group.disarm();
    let _ = child.wait();
}

fn flush(session: &str, label: &str, batch: &str) {
    let text = format!("{label}: {batch}");
    let _ = crate::mail::Mailbox::wake(session, &text);
}
