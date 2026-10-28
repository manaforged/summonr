use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};

use crate::error::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Framing {
    Line,
    ContentLength,
}

pub(crate) enum Read {
    Line(String),
    Oversize,
    Closed,
}

pub(crate) struct Lines<R> {
    reader: BufReader<R>,
    framing: Framing,
    max: usize,
    pending: Vec<u8>,
    over: bool,
}

impl<R: AsyncRead + Unpin> Lines<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self::framed(
            reader,
            Framing::Line,
            crate::limits::limits().mcp_frame_max_bytes,
        )
    }

    pub(crate) fn framed(reader: R, framing: Framing, max: usize) -> Self {
        Self {
            reader: BufReader::new(reader),
            framing,
            max,
            pending: Vec::new(),
            over: false,
        }
    }

    pub(crate) async fn next(&mut self) -> std::io::Result<Read> {
        match self.framing {
            Framing::Line => self.line().await,
            Framing::ContentLength => self.sized().await,
        }
    }

    async fn line(&mut self) -> std::io::Result<Read> {
        loop {
            let chunk = self.reader.fill_buf().await?;
            if chunk.is_empty() {
                return Ok(Read::Closed);
            }
            let (take, done) = chunk
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or((chunk.len(), false), |at| (at + 1, true));
            if !self.over && self.pending.len() + take <= self.max {
                self.pending.extend_from_slice(&chunk[..take]);
            } else if !self.over {
                self.over = true;
                self.pending = Vec::new();
            }
            self.reader.consume(take);
            if done {
                break;
            }
        }
        let line = std::mem::take(&mut self.pending);
        if std::mem::take(&mut self.over) {
            return Ok(Read::Oversize);
        }
        Ok(Read::Line(text(line)))
    }

    async fn sized(&mut self) -> std::io::Result<Read> {
        let mut length = None;
        loop {
            let mut header = String::new();
            if self.reader.read_line(&mut header).await? == 0 {
                return Ok(Read::Closed);
            }
            if header == "\r\n" || header == "\n" {
                break;
            }
            if let Some(value) = header
                .to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::trim)
            {
                length = value.parse::<usize>().ok();
            }
        }
        let length = length.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "message missing Content-Length",
            )
        })?;
        let mut body = (&mut self.reader).take(length as u64);
        if length > self.max {
            tokio::io::copy(&mut body, &mut tokio::io::sink()).await?;
            return Ok(Read::Oversize);
        }
        let mut bytes = Vec::with_capacity(length);
        body.read_to_end(&mut bytes).await?;
        if bytes.len() < length {
            return Ok(Read::Closed);
        }
        Ok(Read::Line(text(bytes)))
    }
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
}

pub(crate) fn encode(framing: Framing, value: &Value) -> Result<Vec<u8>> {
    let mut body = serde_json::to_vec(value)?;
    match framing {
        Framing::Line => {
            body.push(b'\n');
            Ok(body)
        }
        Framing::ContentLength => {
            let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
            out.append(&mut body);
            Ok(out)
        }
    }
}
