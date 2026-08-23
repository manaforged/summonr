use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::fact::{CommandReceipt, Fact};
use super::id::{CommitId, Digest, ScopedId};

pub(crate) const FORMAT: u16 = 2;
pub(crate) const LINE_LIMIT: usize = 16 * 1_048_576;
pub(crate) const PAYLOAD_LIMIT: usize = 14 * 1_048_576;
pub(crate) const FACT_LIMIT: usize = 4_096;

#[derive(Debug, Error)]
pub(crate) enum EncodingError {
    #[error("format-2 encoding failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("commit payload has {actual} bytes; limit is {limit}")]
    Payload { actual: usize, limit: usize },
    #[error("commit line has {actual} bytes; limit is {limit}")]
    Line { actual: usize, limit: usize },
    #[error("commit has {actual} facts; limit is {limit}")]
    Facts { actual: usize, limit: usize },
    #[error("commit digest does not match its canonical body")]
    Digest,
    #[error("journal line is empty")]
    Empty,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Commit {
    pub(crate) format: u16,
    pub(crate) seq: u64,
    pub(crate) id: CommitId,
    pub(crate) previous: Option<Digest>,
    pub(crate) command: Option<CommandReceipt>,
    pub(crate) facts: Vec<Fact>,
    pub(crate) digest: Digest,
}

impl Commit {
    pub(crate) fn new(
        seq: u64,
        id: CommitId,
        previous: Option<Digest>,
        command: Option<CommandReceipt>,
        facts: Vec<Fact>,
    ) -> Result<Self, EncodingError> {
        let mut commit = Self {
            format: FORMAT,
            seq,
            id,
            previous,
            command,
            facts,
            digest: Digest::default(),
        };
        commit.digest = commit.calculated_digest()?;
        commit.validate_size()?;
        Ok(commit)
    }

    pub(crate) fn calculated_digest(&self) -> Result<Digest, EncodingError> {
        let body = self.canonical_body()?;
        Ok(Digest::hash(b"mage:commit:v2", &body))
    }

    pub(crate) fn validate_digest(&self) -> Result<(), EncodingError> {
        if self.digest != self.calculated_digest()? {
            return Err(EncodingError::Digest);
        }
        Ok(())
    }

    pub(crate) fn canonical_body(&self) -> Result<Vec<u8>, EncodingError> {
        if self.facts.len() > FACT_LIMIT {
            return Err(EncodingError::Facts {
                actual: self.facts.len(),
                limit: FACT_LIMIT,
            });
        }
        let command = self.command.as_ref().map(serde_json::to_vec).transpose()?;
        let facts: Vec<Vec<u8>> = self
            .facts
            .iter()
            .map(serde_json::to_vec)
            .collect::<Result<_, _>>()?;
        let payload =
            command.as_ref().map_or(0, Vec::len) + facts.iter().map(Vec::len).sum::<usize>();
        if payload > PAYLOAD_LIMIT {
            return Err(EncodingError::Payload {
                actual: payload,
                limit: PAYLOAD_LIMIT,
            });
        }

        let mut output = Vec::with_capacity(payload.saturating_add(128));
        output.extend_from_slice(&self.format.to_be_bytes());
        output.extend_from_slice(&self.seq.to_be_bytes());
        push_scoped(&mut output, self.id);
        push_digest(&mut output, self.previous);
        match command {
            Some(command) => {
                output.push(1);
                push_frame(&mut output, &command);
            }
            None => output.push(0),
        }
        push_len(&mut output, facts.len());
        for fact in facts {
            push_frame(&mut output, &fact);
        }
        Ok(output)
    }

    pub(crate) fn line(&self) -> Result<Vec<u8>, EncodingError> {
        self.validate_digest()?;
        let mut line = serde_json::to_vec(self)?;
        line.push(b'\n');
        if line.len() > LINE_LIMIT {
            return Err(EncodingError::Line {
                actual: line.len(),
                limit: LINE_LIMIT,
            });
        }
        Ok(line)
    }

    pub(crate) fn parse(line: &[u8]) -> Result<Self, EncodingError> {
        if line.is_empty() {
            return Err(EncodingError::Empty);
        }
        if line.len().saturating_add(1) > LINE_LIMIT {
            return Err(EncodingError::Line {
                actual: line.len().saturating_add(1),
                limit: LINE_LIMIT,
            });
        }
        let commit: Self = serde_json::from_slice(line)?;
        commit.validate_size()?;
        commit.validate_digest()?;
        Ok(commit)
    }

    fn validate_size(&self) -> Result<(), EncodingError> {
        let _body = self.canonical_body()?;
        let line = serde_json::to_vec(self)?.len().saturating_add(1);
        if line > LINE_LIMIT {
            return Err(EncodingError::Line {
                actual: line,
                limit: LINE_LIMIT,
            });
        }
        Ok(())
    }
}

fn push_scoped<K>(output: &mut Vec<u8>, id: ScopedId<K>) {
    output.extend_from_slice(id.session().as_bytes());
    output.extend_from_slice(id.local().as_bytes());
}

fn push_digest(output: &mut Vec<u8>, digest: Option<Digest>) {
    match digest {
        Some(digest) => {
            output.push(1);
            output.extend_from_slice(digest.as_bytes());
        }
        None => output.push(0),
    }
}

fn push_frame(output: &mut Vec<u8>, frame: &[u8]) {
    push_len(output, frame.len());
    output.extend_from_slice(frame);
}

fn push_len(output: &mut Vec<u8>, value: usize) {
    output.extend_from_slice(&u64::try_from(value).unwrap_or(u64::MAX).to_be_bytes());
}
