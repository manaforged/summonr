use std::path::PathBuf;
use std::process::Stdio;

use tokio::process::{Child, Command};

use crate::error::{Error, Result};

use super::sandbox::Sandbox;
use super::{
    Recent, ToolOutput, ToolRegistry, object_bool, object_string, object_usize, parse_args,
};

const LOG_READ_BYTES: usize = 512 * 1024;
const TAIL_LINES: usize = 60;
const SHELL_DEFAULT_SECS: u64 = 120;
const SHELL_MAX_SECS: u64 = 600;

const REPEAT_SECS: u64 = 45;

pub(crate) fn exit_code(output: &str) -> Option<i32> {
    output
        .lines()
        .take(4)
        .find_map(|line| line.strip_prefix("exit ")?.trim().parse().ok())
}

struct StartedCommand {
    id: String,
    pid: u32,
    log_path: PathBuf,
    exit_path: PathBuf,
    child: Child,
}

impl ToolRegistry {
    pub(super) async fn run_command(&self, arguments: &str) -> Result<ToolOutput> {
        let args: serde_json::Value = parse_args("run_command", arguments)?;
        if let Some(id) = args
            .get("job")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
        {
            if object_bool(&args, "kill") {
                let record = crate::jobs::load(&self.workspace, id)?;
                kill_group(record.pid);
                let exit = crate::jobs::exit_path(&self.workspace, id);
                if !exit.is_file() {
                    crate::jobs::write_killed(&exit)?;
                }
            }
            let status = crate::jobs::status(&self.workspace, id)?;
            return Ok(ToolOutput {
                success: status.running || status.exit_code == Some(0),
                text: status.text,
                mutated: false,
            });
        }
        let command = object_string(&args, "command")?;
        self.check_command(command)?;
        let background = object_bool(&args, "background");
        let fresh = object_bool(&args, "fresh");
        let writes = self.writes.load(std::sync::atomic::Ordering::Relaxed);
        if !background
            && !fresh
            && let Some(output) = self.repeated(command, writes)
        {
            return Ok(output);
        }
        let started = self.start_command(command, background)?;
        if background {
            return Ok(background_result(started));
        }
        let timeout = object_usize(&args, "timeout_secs")
            .map(|secs| u64::try_from(secs).unwrap_or(SHELL_DEFAULT_SECS))
            .unwrap_or(SHELL_DEFAULT_SECS)
            .clamp(1, SHELL_MAX_SECS);
        self.wait_command(started, command, writes, timeout).await
    }

    fn check_command(&self, command: &str) -> Result<()> {
        self.policy.command(command, &self.workspace)
    }

    fn repeated(&self, command: &str, writes: u64) -> Option<ToolOutput> {
        let recent = self.recent.lock().ok()?;
        let last = recent.as_ref()?;
        if last.command == command
            && last.writes == writes
            && last.at.elapsed().as_secs() < REPEAT_SECS
        {
            Some(ToolOutput {
                text: format!(
                    "(same command {}s ago, no edits since — result repeated, not re-run)\n{}",
                    last.at.elapsed().as_secs(),
                    last.output
                ),
                success: last.success,
                mutated: false,
            })
        } else {
            None
        }
    }

    fn start_command(&self, command: &str, background: bool) -> Result<StartedCommand> {
        let (id, rec_path, log_path, exit_path) = crate::jobs::prepare(&self.workspace)?;
        let script = crate::jobs::wrap(command, &exit_path);
        let child = spawn_command(
            &self.workspace,
            &self.env,
            self.sandbox.as_ref(),
            &script,
            &log_path,
            background,
        )?;
        let pid = child
            .id()
            .ok_or_else(|| Error::Tool("command started without a pid".to_string()))?;
        crate::jobs::write(
            &rec_path,
            &crate::jobs::stamp(
                &id,
                pid,
                command,
                &self.workspace,
                background,
                self.session.lock().ok().and_then(|session| session.clone()),
            )?,
        )?;
        Ok(StartedCommand {
            id,
            pid,
            log_path,
            exit_path,
            child,
        })
    }

    async fn wait_command(
        &self,
        started: StartedCommand,
        command: &str,
        writes: u64,
        timeout: u64,
    ) -> Result<ToolOutput> {
        let StartedCommand {
            id: _,
            pid,
            log_path,
            exit_path,
            mut child,
        } = started;
        match tokio::time::timeout(std::time::Duration::from_secs(timeout), child.wait()).await {
            Ok(Ok(status)) => {
                let code = status.code().unwrap_or(-1);
                crate::jobs::write_exit(&exit_path, code)?;
                let raw = crate::jobs::preview(&log_path, LOG_READ_BYTES)?;
                let output = format!("exit {code}\n{}", super::digest(command, &raw));
                if let Ok(mut recent) = self.recent.lock() {
                    *recent = Some(Recent {
                        command: command.to_string(),
                        at: std::time::Instant::now(),
                        writes,
                        output: output.clone(),
                        success: code == 0,
                    });
                }
                Ok(ToolOutput {
                    text: output,
                    success: code == 0,
                    mutated: code == 0 && !readonly_verb(command),
                })
            }
            Ok(Err(error)) => Err(Error::Tool(format!("command failed: {error}"))),
            Err(_) => {
                kill_group(pid);
                Err(Error::Tool(format!("command timed out after {timeout}s")))
            }
        }
    }
}

fn spawn_command(
    workspace: &std::path::Path,
    env: &[(String, String)],
    sandbox: Option<&Sandbox>,
    script: &str,
    log_path: &std::path::Path,
    background: bool,
) -> Result<Child> {
    let log = std::fs::File::create(log_path)
        .map_err(|error| Error::Tool(format!("cannot create job log: {error}")))?;
    let err = log
        .try_clone()
        .map_err(|error| Error::Tool(format!("cannot clone job log: {error}")))?;
    let mut cmd = if let Some(sandbox) = sandbox {
        sandbox.command("bash", &["-o", "pipefail", "-c", script], workspace)
    } else {
        let mut cmd = Command::new("bash");
        cmd.args(["-o", "pipefail", "-c", script])
            .current_dir(workspace);
        cmd
    };
    cmd.env_clear()
        .envs(command_env())
        .envs(env.iter().cloned())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(err))
        .kill_on_drop(!background)
        .stdin(Stdio::null());
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    cmd.spawn()
        .map_err(|error| Error::Tool(format!("cannot start command: {error}")))
}

fn background_result(started: StartedCommand) -> ToolOutput {
    let StartedCommand {
        id,
        pid,
        log_path,
        child,
        ..
    } = started;
    drop(child);
    ToolOutput {
        text: format!(
            "started job {id}\npid {pid}\nlog {}\nstate: running",
            log_path.display()
        ),
        success: true,
        mutated: false,
    }
}

pub(super) fn digest(command: &str, raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.len() <= TAIL_LINES {
        return raw.trim_end().to_string();
    }
    if is_toolchain(command)
        && let Some(output) = toolchain_output(&lines)
    {
        return output;
    }
    plain_output(&lines)
}

fn is_toolchain(command: &str) -> bool {
    [
        "cargo",
        "rustc",
        "npm",
        "node --test",
        "pytest",
        "go test",
        "go build",
        "tsc",
    ]
    .iter()
    .any(|tool| command.contains(tool))
}

fn toolchain_output(lines: &[&str]) -> Option<String> {
    let picked = toolchain_digest(lines);
    if picked.is_empty() {
        return None;
    }
    let mut out = picked.join("\n");
    out.push_str(&format!(
        "\n[{} lines captured; {} shown — errors, failures, and summaries]",
        lines.len(),
        picked.len()
    ));
    Some(out)
}

fn plain_output(lines: &[&str]) -> String {
    let head = &lines[..10];
    let tail = &lines[lines.len() - (TAIL_LINES - 10)..];
    format!(
        "{}\n[... {} lines captured ...]\n{}",
        head.join("\n"),
        lines.len() - TAIL_LINES,
        tail.join("\n")
    )
}

fn toolchain_digest(lines: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut carry = 0_usize;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let starts_block = trimmed.starts_with("error")
            || trimmed.starts_with("thread '")
            || trimmed.starts_with("failures:")
            || trimmed.starts_with("---- ")
            || trimmed.starts_with("FAIL")
            || trimmed.starts_with("not ok")
            || trimmed.starts_with("✗")
            || trimmed.contains("panicked at");
        let summary = trimmed.starts_with("test result:")
            || trimmed.starts_with("warning: unused")
            || trimmed.starts_with("warning: `")
            || trimmed.starts_with("Finished")
            || trimmed.starts_with("could not compile")
            || trimmed.starts_with("Tests:")
            || trimmed.starts_with("# tests")
            || trimmed.starts_with("# pass")
            || trimmed.starts_with("# fail")
            || trimmed.ends_with(" FAILED")
            || trimmed.ends_with(" ... FAILED");
        if starts_block {
            carry = 12;
        }
        if carry > 0 {
            out.push((*line).to_string());
            carry -= 1;
            if trimmed.is_empty()
                && index + 1 < lines.len()
                && !lines[index + 1].trim_start().starts_with(' ')
            {
                carry = 0;
            }
        } else if summary {
            out.push((*line).to_string());
        }
        if out.len() > 400 {
            out.push("[digest truncated]".to_string());
            break;
        }
    }
    out
}

fn command_env() -> Vec<(String, String)> {
    const KEEP: &[&str] = &[
        "PATH",
        "HOME",
        "TMPDIR",
        "TMP",
        "TEMP",
        "LANG",
        "LC_ALL",
        "TERM",
        "SHELL",
        "USER",
        "LOGNAME",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "CARGO_TARGET_DIR",
    ];
    KEEP.iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|value| ((*key).to_string(), value))
        })
        .collect()
}

fn readonly_verb(command: &str) -> bool {
    let mut words = command.split_whitespace();
    let Some(verb) = words.next() else {
        return true;
    };
    let verb = verb.rsplit('/').next().unwrap_or(verb);
    match verb {
        "cat" | "ls" | "grep" | "rg" | "find" | "head" | "tail" | "wc" | "echo" | "printf"
        | "pwd" | "which" | "env" | "printenv" | "type" | "file" | "stat" | "du" | "df"
        | "date" | "true" | "false" | "test" | "tree" | "less" | "more" | "sort" | "uniq"
        | "diff" | "cut" | "awk" | "sed" | "jq" | "xxd" | "od" => true,
        "git" => matches!(
            words.next(),
            Some(
                "status" | "log" | "diff" | "show" | "branch" | "ls-files" | "blame" | "rev-parse"
            )
        ),
        _ => false,
    }
}

fn kill_group(pid: u32) {
    #[cfg(unix)]
    {
        let silent = |args: &[&str]| {
            drop(
                std::process::Command::new("kill")
                    .args(args)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status(),
            );
        };
        silent(&["-KILL", &format!("-{pid}")]);
        silent(&["-KILL", &pid.to_string()]);
        drop(
            std::process::Command::new("pkill")
                .args(["-KILL", "-P", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
        );
    }
    #[cfg(not(unix))]
    let _ = pid;
}
