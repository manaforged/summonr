use std::path::PathBuf;
use std::process::Stdio;

#[cfg(windows)]
use process_wrap::tokio::{ChildWrapper, CommandWrap, JobObject};
#[cfg(unix)]
use tokio::process::Child;
use tokio::process::Command;

use crate::error::{Error, Result};
use crate::policy::Grant;

use super::sandbox::Sandbox;
use super::{ToolOutput, ToolRegistry, object_bool, object_string, object_usize, parse_args};

const LOG_READ_BYTES: usize = 512 * 1024;
const TAIL_LINES: usize = 60;

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
    child: CommandChild,
    temp: Option<tempfile::TempDir>,
}

#[cfg(unix)]
type CommandChild = Child;

#[cfg(windows)]
struct CommandChild {
    child: Box<dyn ChildWrapper>,
    kill_on_drop: bool,
    sandbox: bool,
}

#[cfg(windows)]
impl CommandChild {
    fn id(&self) -> Option<u32> {
        self.child.id()
    }

    async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    fn start_kill(&mut self) -> std::io::Result<()> {
        if self.sandbox {
            return self
                .child
                .id()
                .map_or(Ok(()), |pid| super::sandbox::windows::stop(pid).map(drop));
        }
        self.child.start_kill()
    }
}

#[cfg(windows)]
impl Drop for CommandChild {
    fn drop(&mut self) {
        if self.kill_on_drop
            && let Err(error) = self.start_kill()
        {
            eprintln!("command cleanup failed: {error}");
        }
    }
}

struct ProcessGroup {
    pid: u32,
    armed: bool,
    sandbox: bool,
}

impl ProcessGroup {
    fn new(pid: u32, sandbox: bool) -> Self {
        Self {
            pid,
            armed: true,
            sandbox,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if self.armed {
            kill_group(self.pid, self.sandbox);
        }
    }
}

impl ToolRegistry {
    pub(super) async fn run_command(&self, arguments: &str) -> Result<ToolOutput> {
        let args: serde_json::Value = parse_args("run_command", arguments)?;
        let plan = self.policy.is_read_only();
        let restricted = self.policy.restricts_writes();
        let read_only = plan || restricted;
        if let Some(id) = args
            .get("job")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
        {
            if plan {
                return Err(Error::Denied(
                    "Plan mode does not control background commands".to_string(),
                ));
            }
            if object_bool(&args, "kill") {
                let record = crate::jobs::load(&self.workspace, id)?;
                let workspace = self.workspace.clone();
                tokio::task::spawn_blocking(move || stop_job(&workspace, &record))
                    .await
                    .map_err(|error| Error::Tool(format!("job stop failed: {error}")))??;
            }
            let status = crate::jobs::status(&self.workspace, id)?;
            return Ok(ToolOutput {
                success: status.running || status.exit_code == Some(0),
                text: status.text,
                mutated: false,
                grant: None,
            });
        }
        let command = object_string(&args, "command")?;
        let approved = self.take_approval();
        if !approved {
            self.check_command(command)?;
        }
        let background = object_bool(&args, "background");
        if read_only && self.sandbox.is_none() {
            return Err(Error::Denied(
                if plan {
                    "Plan mode requires the command sandbox for shell inspection"
                } else {
                    "a restricted write policy requires the command sandbox for shell inspection"
                }
                .to_string(),
            ));
        }
        if read_only && background {
            return Err(Error::Denied(
                if plan {
                    "Plan mode does not permit background commands"
                } else {
                    "restricted write policy does not permit background commands"
                }
                .to_string(),
            ));
        }
        if background && !self.background {
            return Err(Error::Denied(
                "background commands are unavailable in child sessions".to_string(),
            ));
        }
        let started = self.start_command(command, background, read_only, approved)?;
        if background {
            return Ok(background_result(started));
        }
        let timeout = object_usize(&args, "timeout_secs").and_then(|secs| u64::try_from(secs).ok());
        self.wait_command(started, command, timeout, read_only, !approved)
            .await
    }

    pub(crate) fn stop_jobs(&self) -> Result<()> {
        let session = self
            .session
            .lock()
            .map_err(|error| Error::Tool(format!("cannot read tool session: {error}")))?;
        let Some(session) = session.clone() else {
            return Ok(());
        };
        for record in crate::jobs::since(&self.workspace, 0, None)
            .into_iter()
            .filter(|record| record.session.as_deref() == Some(&session))
        {
            if crate::jobs::status(&self.workspace, &record.id)?.running {
                stop_job(&self.workspace, &record)?;
            }
        }
        Ok(())
    }

    fn check_command(&self, command: &str) -> Result<()> {
        self.policy.command(command, &self.workspace)
    }

    fn start_command(
        &self,
        command: &str,
        background: bool,
        read_only: bool,
        approved: bool,
    ) -> Result<StartedCommand> {
        let temp = if background {
            None
        } else {
            Some(tempfile::tempdir()?)
        };
        let job_workspace = temp
            .as_ref()
            .map_or(self.workspace.as_path(), tempfile::TempDir::path);
        let (id, rec_path, log_path, exit_path) = crate::jobs::prepare(job_workspace)?;
        let script = if background {
            crate::shell::wrap(command, &exit_path, &self.workspace)
        } else {
            command.to_string()
        };
        let one_shot = self.sandbox.is_some()
            && self
                .network_once
                .swap(false, std::sync::atomic::Ordering::SeqCst);
        let sandbox = if approved {
            None
        } else if one_shot {
            self.sandbox
                .as_ref()
                .map(Sandbox::allow_network)
                .transpose()?
        } else {
            self.sandbox.clone()
        };
        let child = spawn_command(
            &self.workspace,
            &self.env,
            sandbox.as_ref(),
            &script,
            &log_path,
            background,
            read_only,
        )?;
        let pid = child
            .id()
            .ok_or_else(|| Error::Tool("command started without a pid".to_string()))?;
        if background {
            let mut record = crate::jobs::stamp(
                &id,
                pid,
                command,
                &self.workspace,
                true,
                self.session.lock().ok().and_then(|session| session.clone()),
            )?;
            record.sandbox = self.sandbox.is_some();
            crate::jobs::write(&rec_path, &record)?;
        }
        Ok(StartedCommand {
            id,
            pid,
            log_path,
            exit_path,
            child,
            temp,
        })
    }

    async fn wait_command(
        &self,
        started: StartedCommand,
        command: &str,
        timeout: Option<u64>,
        read_only: bool,
        contained: bool,
    ) -> Result<ToolOutput> {
        let StartedCommand {
            id: _,
            pid,
            log_path,
            exit_path,
            mut child,
            temp: _temp,
        } = started;
        let mut group = ProcessGroup::new(pid, contained);
        let started = std::time::Instant::now();
        let waited = match timeout {
            Some(seconds) => {
                tokio::time::timeout(
                    std::time::Duration::from_secs(seconds),
                    wait_shell(&mut child),
                )
                .await
            }
            None => Ok(wait_shell(&mut child).await),
        };
        match waited {
            Ok(Ok(status)) => {
                stop_command(&mut child, pid).map_err(|error| {
                    Error::Tool(format!("command descendant cleanup failed: {error}"))
                })?;
                settle_command(&mut child).await.map_err(|error| {
                    Error::Tool(format!("command descendant cleanup failed: {error}"))
                })?;
                group.disarm();
                let code = status.code().unwrap_or(-1);
                crate::jobs::write_exit(&exit_path, code)?;
                let raw = crate::jobs::preview(&log_path, LOG_READ_BYTES)?;
                let grant = (code != 0)
                    .then(|| sandbox_grant(self, command, &raw))
                    .flatten();
                let output = format!(
                    "exit {code}\n{}{}{}{}",
                    super::digest(command, &raw),
                    slow_search_note(command, started.elapsed().as_secs()),
                    plan_write_note(read_only, &raw),
                    sandbox_note(grant.as_ref(), self, &raw)
                );
                Ok(ToolOutput {
                    text: output,
                    success: code == 0,
                    mutated: !read_only && code == 0 && !readonly_verb(command),
                    grant,
                })
            }
            Ok(Err(error)) => Err(Error::Tool(format!("command failed: {error}"))),
            Err(_) => {
                stop_command(&mut child, pid)
                    .map_err(|error| Error::Tool(format!("command cleanup failed: {error}")))?;
                let status = child
                    .wait()
                    .await
                    .map_err(|error| Error::Tool(format!("command cleanup failed: {error}")))?;
                group.disarm();
                crate::jobs::write_exit(&exit_path, status.code().unwrap_or(-1))?;
                let raw = crate::jobs::preview(&log_path, LOG_READ_BYTES)?;
                let timeout = timeout.map_or_else(
                    || "command timed out".to_string(),
                    |seconds| format!("command timed out after {seconds}s"),
                );
                let warning = if read_only {
                    "workspace writes were restricted by the command sandbox"
                } else {
                    "the command may have changed workspace files before timeout"
                };
                Ok(ToolOutput {
                    text: format!("{timeout}\n{warning}\n{}", super::digest(command, &raw)),
                    success: false,
                    mutated: !read_only,
                    grant: None,
                })
            }
        }
    }
}

fn plan_write_note(read_only: bool, raw: &str) -> String {
    if read_only && raw.contains("Operation not permitted") {
        "\nplan mode runs the shell read-only. Return the plan in your reply. Switch to auto mode before building or editing files."
            .to_string()
    } else {
        String::new()
    }
}

fn sandbox_grant(registry: &ToolRegistry, command: &str, raw: &str) -> Option<Grant> {
    let eperm = raw.contains("Operation not permitted") || raw.contains("Permission denied");
    if eperm
        && let Some(grant) = crate::policy::outside_write(
            command,
            &registry.workspace,
            &registry.policy.write_roots(),
        )
    {
        return Some(grant);
    }
    if eperm && let Some(path) = crate::policy::implicit_write(command) {
        return Some(Grant::Write(path));
    }
    if registry.denies_network() && (network_text(raw) || (eperm && network_verb(command))) {
        return Some(Grant::Network);
    }
    None
}

fn network_text(raw: &str) -> bool {
    const MARKERS: [&str; 8] = [
        "Could not resolve host",
        "Network is unreachable",
        "Temporary failure in name resolution",
        "nodename nor servname",
        "Failed to connect",
        "Could not connect",
        "Connection refused",
        "curl: (6)",
    ];
    MARKERS.iter().any(|marker| raw.contains(marker))
}

fn network_verb(command: &str) -> bool {
    command
        .split_whitespace()
        .find(|word| !word.contains('='))
        .map(|word| {
            word.rsplit(['/', '\\'])
                .next()
                .unwrap_or(word)
                .trim_matches(['\'', '"', '`'])
                .to_ascii_lowercase()
        })
        .is_some_and(|verb| {
            matches!(
                verb.strip_suffix(".exe").unwrap_or(&verb),
                "curl" | "wget" | "ssh" | "scp" | "sftp"
            )
        })
}

fn sandbox_note(grant: Option<&Grant>, registry: &ToolRegistry, raw: &str) -> String {
    match grant {
        Some(Grant::Network) => {
            "\nnote: network is off in the sandbox. Ask the user to run /net on for this session or /net once, then retry.\n"
                .to_string()
        }
        Some(Grant::Write(path)) => format!(
            "\nnote: the OS sandbox denied a write outside the workspace. Ask the user to approve the directory with /write {}, or add it to [sandbox] write in ~/.mage/config.toml.\n",
            path.display()
        ),
        None if registry.has_sandbox()
            && (raw.contains("Operation not permitted") || raw.contains("Permission denied")) =>
        {
            if registry.denies_network() {
                "\nnote: the OS sandbox denied this command (network is off; writes outside the workspace need /write PATH).\n"
                    .to_string()
            } else {
                "\nnote: the OS sandbox denied a write outside the workspace. Ask the user to approve the directory with /write PATH, or add it to [sandbox] write in ~/.mage/config.toml.\n"
                    .to_string()
            }
        }
        None => String::new(),
    }
}

fn slow_search_note(command: &str, secs: u64) -> String {
    let searches = command.contains("grep -r")
        || command.contains("grep -R")
        || command.starts_with("find ")
        || command.contains("; find ")
        || command.contains("&& find ");
    if secs < 30 || !searches {
        return String::new();
    }
    format!(
        "\nnote: this search ran {secs}s; the grep tool skips ignored directories such as target/ and node_modules/"
    )
}

fn spawn_command(
    workspace: &std::path::Path,
    env: &[(String, String)],
    sandbox: Option<&Sandbox>,
    script: &str,
    log_path: &std::path::Path,
    background: bool,
    read_only: bool,
) -> Result<CommandChild> {
    let log = std::fs::File::create(log_path)
        .map_err(|error| Error::Tool(format!("cannot create job log: {error}")))?;
    let err = log
        .try_clone()
        .map_err(|error| Error::Tool(format!("cannot clone job log: {error}")))?;
    let shell = crate::shell::program()?;
    let args = crate::shell::arguments(script, workspace);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut cmd = if let Some(sandbox) = sandbox {
        if read_only {
            sandbox.read_command(&shell, &args, workspace)
        } else {
            sandbox.command(&shell, &args, workspace)
        }
    } else {
        let mut cmd = Command::new(&shell);
        cmd.args(&args).current_dir(workspace);
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
    #[cfg(unix)]
    let child = cmd.spawn();
    #[cfg(windows)]
    let child = if background || sandbox.is_some() {
        cmd.kill_on_drop(false);
        if background && sandbox.is_some() {
            cmd.env("SUMMONER_SANDBOX_DETACHED", "1");
        }
        cmd.spawn().map(|child| CommandChild {
            child: Box::new(child),
            kill_on_drop: !background,
            sandbox: sandbox.is_some(),
        })
    } else {
        let mut command = CommandWrap::from(cmd);
        command.wrap(JobObject);
        command.spawn().map(|child| CommandChild {
            child,
            kill_on_drop: true,
            sandbox: false,
        })
    };
    child.map_err(|error| Error::Tool(format!("cannot start command: {error}")))
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
        grant: None,
    }
}

pub(super) fn digest(command: &str, raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.len() <= TAIL_LINES {
        return raw.trim_end().to_string();
    }
    if crate::language::toolchain(command)
        && let Some(output) = toolchain_output(&lines)
    {
        return output;
    }
    plain_output(&lines)
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
        "SystemRoot",
        "ComSpec",
        "PATHEXT",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
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

fn kill_group(pid: u32, sandbox: bool) {
    #[cfg(unix)]
    {
        let _ = sandbox;
        let silent = |args: &[&str]| {
            drop(
                std::process::Command::new("kill")
                    .args(args)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status(),
            );
        };
        silent(&["-KILL", "--", &format!("-{pid}")]);
        silent(&["-KILL", &pid.to_string()]);
        drop(
            std::process::Command::new("pkill")
                .args(["-KILL", "-P", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
        );
    }
    #[cfg(windows)]
    {
        if sandbox {
            if let Err(error) = super::sandbox::windows::stop(pid) {
                eprintln!("sandbox cleanup failed: {error}");
            }
            return;
        }
        drop(
            std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
        );
    }
}

fn stop_command(child: &mut CommandChild, pid: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let _ = child;
        kill_group(pid, false);
        Ok(())
    }
    #[cfg(windows)]
    {
        let _ = pid;
        child.start_kill()
    }
}

async fn wait_shell(child: &mut CommandChild) -> std::io::Result<std::process::ExitStatus> {
    #[cfg(unix)]
    {
        child.wait().await
    }
    #[cfg(windows)]
    {
        child.child.inner_mut().wait().await
    }
}

async fn settle_command(child: &mut CommandChild) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let _ = child;
        Ok(())
    }
    #[cfg(windows)]
    {
        child.wait().await.map(drop)
    }
}

fn stop_job(workspace: &std::path::Path, record: &crate::jobs::Record) -> Result<()> {
    #[cfg(windows)]
    if record.sandbox {
        if let Some(helper) = super::sandbox::windows::stop(record.pid)? {
            super::sandbox::windows::wait(&helper)?;
        }
    } else {
        kill_group(record.pid, false);
    }
    #[cfg(not(windows))]
    kill_group(record.pid, record.sandbox);
    let exit = crate::jobs::exit_path(workspace, &record.id);
    if !exit.is_file() {
        crate::jobs::write_killed(&exit)?;
    }
    Ok(())
}
