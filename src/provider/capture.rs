use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::endpoint::Wire;

const DIR: &str = "SUMMONR_CAPTURE_DIR";
static NEXT: AtomicU64 = AtomicU64::new(0);

fn pick(raw: Option<String>) -> Option<PathBuf> {
    let raw = raw?;
    let path = PathBuf::from(raw.trim());
    if path.as_os_str().is_empty() {
        return None;
    }
    std::fs::create_dir_all(&path).ok()?;
    Some(path)
}

pub(super) fn directory() -> Option<PathBuf> {
    pick(std::env::var(DIR).ok())
}

pub(super) fn wire_name(wire: Wire) -> &'static str {
    match wire {
        Wire::Anthropic => "anthropic",
        Wire::Gemini => "gemini",
        Wire::Responses => "responses",
        Wire::Completions => "completions",
        Wire::Decisions => "decisions",
        Wire::AgentSdk => "agent-sdk",
    }
}

#[derive(Clone)]
pub(super) struct Sink {
    path: PathBuf,
}

impl Sink {
    pub(super) fn open(wire: Wire, kind: &str) -> Option<Self> {
        Self::open_in(directory()?, wire, kind)
    }

    fn open_in(dir: PathBuf, wire: Wire, kind: &str) -> Option<Self> {
        std::fs::create_dir_all(&dir).ok()?;
        let run = NEXT.fetch_add(1, Ordering::Relaxed);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs())
            .unwrap_or_default();
        let path = dir.join(format!("{}-{stamp}-{run:04}-{kind}.jsonl", wire_name(wire)));
        eprintln!(
            "summonr: capturing {} frames to {}",
            wire_name(wire),
            path.display()
        );
        Some(Self { path })
    }

    pub(super) fn write(&self, label: &str, payload: &str) {
        let line = serde_json::json!({"at": label, "body": payload});
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            && let Ok(text) = serde_json::to_string(&line)
        {
            drop(writeln!(file, "{text}"));
        }
    }

    #[cfg(test)]
    fn path(&self) -> &PathBuf {
        &self.path
    }
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
