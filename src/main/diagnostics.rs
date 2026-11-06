use std::backtrace::Backtrace;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::json;

#[path = "diagnostics/runs.rs"]
mod runs;
use runs::{lock_path, runs, sweep};

pub(super) struct Log {
    path: PathBuf,
    run: String,
    lock: File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunStatus {
    Active,
    Complete,
    Failed,
    Panic,
    Incomplete,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Active => "active",
            Self::Complete => "complete",
            Self::Failed => "failed",
            Self::Panic => "panic",
            Self::Incomplete => "incomplete",
        })
    }
}

struct RunInfo {
    path: PathBuf,
    status: RunStatus,
}

impl Log {
    pub(super) fn start() -> Option<Self> {
        let run = format!("{}-{}", millis(), std::process::id());
        let home = match crate::summonr_home() {
            Ok(home) => home,
            Err(error) => {
                eprintln!("summonr: cannot create the diagnostic log: {error}");
                return None;
            }
        };
        let dir = home.join("logs");
        if let Err(error) = fs::create_dir_all(&dir) {
            eprintln!("summonr: cannot create the diagnostic log: {error}");
            return None;
        }
        let seen = dir.join("crash-seen");
        let noted = fs::read_to_string(&seen).unwrap_or_default();
        if let Some(latest) = sweep(&dir)
            && !noted.contains(&latest)
        {
            eprintln!(
                "summonr: a previous run ended without an exit record; `summonr doctor --logs` shows it"
            );
            if let Err(error) = append(&seen, &json!(latest), true) {
                eprintln!("summonr: cannot record the crash note: {error}");
            }
        }
        let path = dir.join(format!("{run}.jsonl"));
        let held = crate::lock_file(&path, false)
            .and_then(|held| held.ok_or_else(|| io::Error::from(ErrorKind::WouldBlock)));
        let lock = match held {
            Ok(lock) => lock,
            Err(error) => {
                eprintln!("summonr: cannot lock the diagnostic log: {error}");
                return None;
            }
        };
        let result = append(
            &path,
            &json!({
                "time_unix_ms": millis(),
                "run": run,
                "event": "start",
                "pid": std::process::id(),
                "binary": binary(),
                "version": env!("CARGO_PKG_VERSION"),
                "revision": crate::revision(),
                "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
                "cwd": std::env::current_dir()
                    .ok()
                    .map(|path| path.display().to_string()),
            }),
            false,
        );
        let result = result.and_then(|()| {
            append(
                &path,
                &json!({"time_unix_ms": millis(), "run": run, "event": "exit", "code": 0}),
                false,
            )
        });
        if let Err(error) = result {
            eprintln!("summonr: cannot create the diagnostic log: {error}");
            return None;
        }
        hook(path.clone(), run.clone());
        Some(Self { path, run, lock })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn session(&self, session: &str, journal: &Path, workspace: &Path) {
        if let Err(source) = append(
            &self.path,
            &json!({
                "time_unix_ms": millis(),
                "run": self.run,
                "event": "session",
                "session": session,
                "journal": journal.display().to_string(),
                "workspace": workspace.display().to_string(),
            }),
            false,
        ) {
            eprintln!("summonr: cannot write the diagnostic session record: {source}");
        }
    }

    pub(super) fn finish(self, code: i32, error: Option<&str>) {
        if let Err(source) = append(
            &self.path,
            &json!({
                "time_unix_ms": millis(),
                "run": self.run,
                "event": "exit",
                "code": code,
                "error": error,
            }),
            false,
        ) {
            eprintln!("summonr: cannot write the diagnostic exit record: {source}");
        }
        drop(self.lock);
        drop(fs::remove_file(lock_path(&self.path)));
    }
}

pub(super) fn print_logs(exclude: Option<&Path>) -> crate::Result<i32> {
    let dir = crate::summonr_home()?.join("logs");
    println!("{}", dir.display());
    let runs = runs(&dir, exclude)?;
    if runs.is_empty() {
        println!("no prior runs");
    }
    for run in runs.iter().rev().take(20) {
        println!("{}\t{}", run.status, run.path.display());
    }
    Ok(0)
}

pub(super) fn doctor(exclude: Option<&Path>) -> crate::Result<i32> {
    let home = crate::summonr_home()?;
    let dir = home.join("logs");
    let runs = runs(&dir, exclude)?;
    let incomplete = runs
        .iter()
        .filter(|run| matches!(run.status, RunStatus::Incomplete | RunStatus::Panic))
        .count();
    println!("version\t{}", env!("CARGO_PKG_VERSION"));
    println!("revision\t{}", crate::revision());
    println!("profile\t{}", profile());
    println!("binary\t{}", binary());
    println!("home\t{}", home.display());
    println!("logs\t{}", dir.display());
    println!("incomplete runs\t{incomplete}");
    Ok(i32::from(incomplete > 0))
}

pub(super) fn report(exclude: Option<&Path>) -> crate::Result<i32> {
    let dir = crate::summonr_home()?.join("logs");
    let runs = runs(&dir, exclude)?;
    let Some(run) = runs.iter().rev().find(|run| {
        matches!(
            run.status,
            RunStatus::Panic | RunStatus::Failed | RunStatus::Incomplete
        )
    }) else {
        println!("no crashed or failed run to report");
        return Ok(0);
    };
    let body = fs::read_to_string(&run.path)?;
    let name = run.path.file_name().unwrap_or_default().to_string_lossy();
    println!(
        "summonr {} ({})",
        env!("CARGO_PKG_VERSION"),
        crate::revision()
    );
    println!(
        "{} {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        profile()
    );
    println!("run {name} ({})", run.status);
    println!();
    print!("{}", hidden(&body));
    println!();
    println!(
        "Read it before you post it: it lists this run's events, with your home folder shown as ~."
    );
    Ok(0)
}

fn hidden(text: &str) -> String {
    let mut forms: Vec<String> = std::env::home_dir()
        .into_iter()
        .flat_map(|home| {
            let real = home.canonicalize().unwrap_or_else(|_| home.clone());
            [home, real]
        })
        .flat_map(|path| {
            let plain = path.display().to_string();
            let escaped = serde_json::to_string(&plain).unwrap_or_default();
            [escaped.trim_matches('"').to_string(), plain]
        })
        .filter(|form| form.len() > 1)
        .collect();
    forms.sort_by_key(|form| std::cmp::Reverse(form.len()));
    forms.dedup();
    forms.iter().fold(text.to_string(), |text, form| {
        text.replace(form.as_str(), "~")
    })
}

const fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

#[cfg(unix)]
#[expect(unsafe_code, reason = "std has no dup2 or tcgetattr wrapper")]
pub(crate) fn park_stderr() -> Option<i32> {
    let null = File::options().write(true).open("/dev/null").ok()?;
    // SAFETY: dup and dup2 take plain descriptors; stderr and the open null file are valid here.
    unsafe {
        let saved = libc::dup(libc::STDERR_FILENO);
        (saved >= 0 && libc::dup2(std::os::fd::AsRawFd::as_raw_fd(&null), libc::STDERR_FILENO) >= 0)
            .then_some(saved)
    }
}

#[cfg(unix)]
#[expect(unsafe_code, reason = "std has no dup2 or tcgetattr wrapper")]
pub(crate) fn restore_stderr(saved: Option<i32>) {
    let Some(saved) = saved else { return };
    // SAFETY: saved came from dup in park_stderr, and termios is a plain out buffer for tcgetattr.
    unsafe {
        let mut termios = std::mem::zeroed::<libc::termios>();
        let alive = libc::isatty(saved) == 0 || libc::tcgetattr(saved, &raw mut termios) == 0;
        if alive {
            libc::dup2(saved, libc::STDERR_FILENO);
        }
        libc::close(saved);
    }
}

#[cfg(not(unix))]
pub(crate) fn park_stderr() -> Option<i32> {
    None
}

#[cfg(not(unix))]
pub(crate) fn restore_stderr(_saved: Option<i32>) {}

const HANGUP_EXIT: i32 = 129;

fn hook(path: PathBuf, run: String) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.to_string().contains("failed printing to stderr") {
            std::process::exit(HANGUP_EXIT);
        }
        let result = append(
            &path,
            &json!({
                "time_unix_ms": millis(),
                "run": run,
                "event": "panic",
                "message": info.to_string(),
                "backtrace": Backtrace::force_capture().to_string(),
            }),
            true,
        );
        let note = match result {
            Ok(()) => format!("summonr: crash recorded in {}\n", path.display()),
            Err(error) => format!(
                "summonr: cannot write crash record to {}: {error}\n",
                path.display()
            ),
        };
        drop(io::stderr().write_all(note.as_bytes()));
        previous(info);
    }));
}

fn append(path: &Path, value: &serde_json::Value, durable: bool) -> io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(io::Error::other)?;
    line.push(b'\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&line)?;
    if durable { file.sync_data() } else { Ok(()) }
}

fn millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn binary() -> String {
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| {
            option_env!("CARGO_BIN_NAME")
                .unwrap_or("summonr")
                .to_string()
        })
}
