use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine as _;

use crate::endpoint::Wire;
use summonr_core::error::{Error, ProviderFailure, ProviderFailureKind, Result};

const DIR: &str = "SUMMONR_CAPTURE_DIR";

fn pick(raw: Option<String>) -> Result<Option<PathBuf>> {
    let Some(raw) = raw.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(raw.trim());
    std::fs::create_dir_all(&path)?;
    Ok(Some(path))
}

/// Identifies whether captured bytes precede or follow provider execution.
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    Request,
    Response,
    Chunk,
}

pub(super) struct Sink {
    path: PathBuf,
    session: String,
    exchange: String,
    wire: String,
    next: AtomicU64,
}

impl Sink {
    pub(super) fn open(wire: Wire, kind: &str, session: &str) -> Result<Option<Self>> {
        crate::tools::blocking(|| {
            pick(std::env::var(DIR).ok())?
                .map(|dir| Self::open_in(dir, wire, kind, session))
                .transpose()
        })
    }

    fn open_in(dir: PathBuf, wire: Wire, kind: &str, session: &str) -> Result<Self> {
        let session_path = std::path::Path::new(session);
        if session_path.components().count() != 1
            || !matches!(
                session_path.components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            return Err(crate::Error::Config(
                "capture session must be one path component".to_string(),
            ));
        }
        let dir = dir.join(session);
        std::fs::create_dir_all(&dir)?;
        let exchange = uuid::Uuid::now_v7().to_string();
        let wire = super::wire::spec(wire).name.to_string();
        let path = dir.join(format!("{wire}-{exchange}-{kind}.jsonl"));
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        options.open(&path)?;
        Ok(Self {
            path,
            session: session.to_string(),
            exchange,
            wire,
            next: AtomicU64::new(1),
        })
    }

    pub(super) fn write(&self, kind: Stage, payload: &[u8]) -> Result<()> {
        let result = crate::tools::blocking(|| self.append(kind, payload));
        match kind {
            Stage::Request => result,
            Stage::Response | Stage::Chunk => result.map_err(|error| {
                Error::Provider(ProviderFailure {
                    kind: ProviderFailureKind::Malformed,
                    status: None,
                    message: format!("audit capture failed after provider output: {error}"),
                    partial_output: true,
                    retry_after: None,
                })
            }),
        }
    }

    fn append(&self, kind: Stage, payload: &[u8]) -> Result<()> {
        let _held = crate::durable::store::lock(&self.path, true)?;
        let line = serde_json::json!({
            "schema": 1, "session": self.session, "source": "provider",
            "stream": self.exchange, "sequence": self.next.fetch_add(1, Ordering::Relaxed),
            "record": {
                "at": summonr_core::clock::now_millis(), "kind": kind, "wire": self.wire,
                "encoding": "base64", "body": base64::engine::general_purpose::STANDARD.encode(payload),
            },
        });
        let mut file = std::fs::OpenOptions::new().append(true).open(&self.path)?;
        let before = file.metadata()?.len();
        let mut bytes = serde_json::to_vec(&line)?;
        bytes.push(b'\n');
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_data()) {
            file.set_len(before)?;
            return Err(error.into());
        }
        Ok(())
    }

    #[cfg(test)]
    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
