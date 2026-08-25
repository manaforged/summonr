use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

const FILE: &str = "summoner.jsonl";

pub(super) struct Log {
    path: PathBuf,
    run: String,
}

impl Log {
    pub(super) fn start() -> Option<Self> {
        let run = format!("{}-{}", millis(), std::process::id());
        let home = match summoner::mage_home() {
            Ok(home) => home,
            Err(error) => {
                eprintln!("summoner: cannot create the diagnostic log: {error}");
                return None;
            }
        };
        let dir = home.join("logs");
        let path = dir.join(FILE);
        let result = fs::create_dir_all(dir).and_then(|()| {
            append(
                &path,
                &json!({
                    "time_unix_ms": millis(),
                    "run": run,
                    "event": "start",
                    "pid": std::process::id(),
                    "binary": option_env!("CARGO_BIN_NAME").unwrap_or("summon"),
                    "version": env!("CARGO_PKG_VERSION"),
                    "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
                    "cwd": std::env::current_dir()
                        .ok()
                        .map(|path| path.display().to_string()),
                }),
            )
        });
        if let Err(error) = result {
            eprintln!("summoner: cannot create the diagnostic log: {error}");
            return None;
        }
        hook(path.clone(), run.clone());
        Some(Self { path, run })
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
        ) {
            eprintln!("summoner: cannot write the diagnostic session record: {source}");
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
        ) {
            eprintln!("summoner: cannot write the diagnostic exit record: {source}");
        }
    }
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
        );
        match result {
            Ok(()) => eprintln!("summoner: crash recorded in {}", path.display()),
            Err(error) => eprintln!(
                "summoner: cannot write crash record to {}: {error}",
                path.display()
            ),
        }
        previous(info);
    }));
}

fn append(path: &Path, value: &serde_json::Value) -> io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(io::Error::other)?;
    line.push(b'\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&line)?;
    file.sync_data()
}

fn millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
