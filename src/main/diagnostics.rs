use std::backtrace::Backtrace;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::json;

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

fn paths(dir: &Path, exclude: Option<&Path>) -> io::Result<Vec<PathBuf>> {
    let mut paths = match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "jsonl")
            })
            .filter(|path| exclude != Some(path.as_path()))
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    paths.sort();
    Ok(paths)
}

fn runs(dir: &Path, exclude: Option<&Path>) -> io::Result<Vec<RunInfo>> {
    paths(dir, exclude)?
        .into_iter()
        .map(|path| {
            Ok(RunInfo {
                status: read_status(&path)?,
                path,
            })
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Event {
    Panic,
    Exit,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct Record {
    event: Option<Event>,
    code: Option<i64>,
}

fn read_status(path: &Path) -> io::Result<RunStatus> {
    Ok(status(path, &fs::read(path)?))
}

fn status(path: &Path, bytes: &[u8]) -> RunStatus {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return RunStatus::Incomplete;
    };
    let mut panic = false;
    let mut exit = None;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(record) = serde_json::from_str::<Record>(line) else {
            return RunStatus::Incomplete;
        };
        match record.event {
            Some(Event::Panic) => panic = true,
            Some(Event::Exit) => exit = record.code,
            Some(Event::Other) | None => {}
        }
    }
    if panic {
        RunStatus::Panic
    } else if exit == Some(0) {
        RunStatus::Complete
    } else if exit.is_some() {
        RunStatus::Failed
    } else if active(path) {
        RunStatus::Active
    } else {
        RunStatus::Incomplete
    }
}

fn active(path: &Path) -> bool {
    let Ok(lock) = OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path(path))
    else {
        return false;
    };
    match lock.try_lock() {
        Ok(()) => false,
        Err(fs::TryLockError::WouldBlock) => true,
        Err(fs::TryLockError::Error(_)) => true,
    }
}

fn lock_path(path: &Path) -> PathBuf {
    let mut path = path.as_os_str().to_os_string();
    path.push(OsString::from(".lock"));
    PathBuf::from(path)
}

fn sweep(dir: &Path) -> Option<String> {
    let paths = paths(dir, None).ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok();
    let mut kept = Vec::with_capacity(paths.len());
    for path in paths {
        let age = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .and_then(|modified| {
                modified
                    .duration_since(UNIX_EPOCH)
                    .map_err(io::Error::other)
            });
        let expired = match (now, age) {
            (Some(now), Ok(age)) => now.saturating_sub(age) > summonr::limits::log_retain(),
            _ => false,
        };
        if !expired {
            kept.push((path, None));
            continue;
        }
        match read_status(&path) {
            Ok(RunStatus::Active) => kept.push((path, Some(RunStatus::Active))),
            Ok(_) => {
                drop(fs::remove_file(&path));
                drop(fs::remove_file(lock_path(&path)));
            }
            Err(_) => kept.push((path, None)),
        }
    }
    scan(dir, kept)
}

fn scan(dir: &Path, kept: Vec<(PathBuf, Option<RunStatus>)>) -> Option<String> {
    let marker = dir.join("crash-scanned");
    let scanned = fs::read_to_string(&marker).unwrap_or_default();
    let scanned = scanned.trim();
    let mut latest = None;
    let mut advance = None;
    let mut blocked = false;
    for (path, known) in kept {
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        if name.as_str() <= scanned {
            continue;
        }
        let status = match known {
            Some(status) => Some(status),
            None => read_status(&path).ok(),
        };
        match status {
            Some(RunStatus::Incomplete | RunStatus::Panic) => latest = Some(name.clone()),
            Some(RunStatus::Active) | None => blocked = true,
            Some(RunStatus::Complete | RunStatus::Failed) => {}
        }
        if !blocked {
            advance = Some(name);
        }
    }
    if let Some(name) = advance {
        drop(fs::write(&marker, name));
    }
    latest
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
