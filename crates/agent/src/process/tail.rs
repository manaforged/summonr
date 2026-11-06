use std::sync::{Arc, Mutex, PoisonError};

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::task::{JoinError, JoinHandle};

use super::SECRET_MIN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cap {
    Head(usize),
    Tail(usize),
    Lines(usize),
}

pub(crate) struct Tail {
    kept: Arc<Mutex<Vec<u8>>>,
    drain: Option<JoinHandle<()>>,
}

impl Tail {
    pub(crate) fn spawn(pipe: Option<impl AsyncRead + Unpin + Send + 'static>, cap: Cap) -> Self {
        let kept = Arc::new(Mutex::new(Vec::new()));
        let drain = pipe.map(|pipe| tokio::spawn(capture(pipe, cap, Arc::clone(&kept))));
        Self { kept, drain }
    }

    pub(crate) fn draining(&self) -> bool {
        self.drain.is_some()
    }

    pub(crate) fn abort(&self) {
        if let Some(drain) = &self.drain {
            drain.abort();
        }
    }

    pub(crate) async fn settle(&mut self) -> std::result::Result<(), JoinError> {
        match self.drain.take() {
            Some(drain) => drain.await,
            None => Ok(()),
        }
    }

    pub(crate) fn text(&self) -> String {
        let bytes = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }

    pub(crate) fn lines(&self, keep: usize) -> Vec<String> {
        let lines: Vec<String> = self.text().lines().map(str::to_string).collect();
        lines[lines.len().saturating_sub(keep)..].to_vec()
    }

    pub(crate) fn last_line(&self, secrets: &[String]) -> Option<String> {
        self.text()
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map(|line| {
                summonr_core::text::redact(
                    line.trim(),
                    secrets
                        .iter()
                        .map(String::as_str)
                        .filter(|secret| secret.len() >= SECRET_MIN),
                )
            })
    }
}

impl Drop for Tail {
    fn drop(&mut self) {
        if let Some(drain) = &self.drain {
            drain.abort();
        }
    }
}

async fn capture(mut pipe: impl AsyncRead + Unpin, cap: Cap, kept: Arc<Mutex<Vec<u8>>>) {
    let mut chunk = [0_u8; 1024];
    while let Ok(read) = pipe.read(&mut chunk).await {
        if read == 0 {
            break;
        }
        let mut bytes = kept.lock().unwrap_or_else(PoisonError::into_inner);
        let cut = match cap {
            Cap::Head(max) => {
                let room = max.saturating_sub(bytes.len()).min(read);
                bytes.extend_from_slice(&chunk[..room]);
                0
            }
            Cap::Tail(max) => {
                bytes.extend_from_slice(&chunk[..read]);
                bytes.len().saturating_sub(max)
            }
            Cap::Lines(max) => {
                bytes.extend_from_slice(&chunk[..read]);
                bytes
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|(_, byte)| **byte == b'\n')
                    .nth(max)
                    .map_or(0, |(at, _)| at + 1)
            }
        };
        bytes.drain(..cut);
    }
}
