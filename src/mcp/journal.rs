use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;

use crate::limits::MCP_LOG_MAX;

struct Sink {
    dir: PathBuf,
    file: File,
    written: u64,
}

#[derive(Default)]
pub(super) struct Journal(Mutex<Option<Sink>>);

impl Journal {
    pub(super) fn append(&self, server: &str, level: &str, logger: Option<&str>, data: &Value) {
        let source =
            logger.map_or_else(|| server.to_string(), |logger| format!("{server}/{logger}"));
        let line = format!("{} {source} {level} {data}\n", crate::mail::now());
        crate::tools::blocking(|| self.write(line.as_bytes()));
    }

    fn write(&self, line: &[u8]) {
        let Ok(mut slot) = self.0.lock() else {
            return;
        };
        if slot.as_ref().is_none_or(|sink| sink.written > MCP_LOG_MAX) {
            *slot = slot.take().map_or_else(Sink::open, Sink::rotate);
        }
        if let Some(sink) = slot.as_mut()
            && sink.file.write_all(line).is_ok()
        {
            sink.written += line.len() as u64;
        }
    }
}

impl Sink {
    fn open() -> Option<Self> {
        let dir = crate::prompt::summonr_home().ok()?.join("logs");
        std::fs::create_dir_all(&dir).ok()?;
        let sink = Self::attach(dir)?;
        if sink.written > MCP_LOG_MAX {
            return Self::rotate(sink);
        }
        Some(sink)
    }

    fn rotate(self) -> Option<Self> {
        let Self { dir, file, .. } = self;
        drop(file);
        drop(std::fs::rename(dir.join("mcp.log"), dir.join("mcp.log.1")));
        Self::attach(dir)
    }

    fn attach(dir: PathBuf) -> Option<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("mcp.log"))
            .ok()?;
        let written = file.metadata().map_or(0, |meta| meta.len());
        Some(Self { dir, file, written })
    }
}
