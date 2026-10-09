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
use runs::*;

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
        let home = match summonr::summonr_home() {
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
        let held = summonr::lock_file(&path, false)
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
                "revision": summonr::revision(),
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
            true,
        ) {
            eprintln!("summonr: cannot write the diagnostic exit record: {source}");
        }
        drop(self.lock);
        drop(fs::remove_file(lock_path(&self.path)));
    }
}

pub(super) fn print_logs(exclude: Option<&Path>) -> summonr::Result<i32> {
    let dir = summonr::summonr_home()?.join("logs");
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

pub(super) fn doctor(exclude: Option<&Path>) -> summonr::Result<i32> {
    let home = summonr::summonr_home()?;
    let dir = home.join("logs");
    let runs = runs(&dir, exclude)?;
    let incomplete = runs
        .iter()
        .filter(|run| matches!(run.status, RunStatus::Incomplete | RunStatus::Panic))
        .count();
    println!("version\t{}", env!("CARGO_PKG_VERSION"));
    println!("revision\t{}", summonr::revision());
    println!(
        "profile\t{}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    println!("binary\t{}", binary());
    println!("home\t{}", home.display());
    println!("logs\t{}", dir.display());
    println!("incomplete runs\t{incomplete}");
    Ok(i32::from(incomplete > 0))
}

fn hook(path: PathBuf, run: String) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
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
        match result {
            Ok(()) => eprintln!("summonr: crash recorded in {}", path.display()),
            Err(error) => eprintln!(
                "summonr: cannot write crash record to {}: {error}",
                path.display()
            ),
        }
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
