use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde::de::DeserializeOwned;

use super::Line;

pub(crate) trait Fill: DeserializeOwned {
    fn fill(self) -> Self {
        self
    }
}

impl Fill for Line {
    fn fill(self) -> Self {
        match self {
            Line::Receipt {
                text,
                receipt: Some(receipt),
            } if text.is_empty() => Line::Receipt {
                text: crate::verify::context(&receipt),
                receipt: Some(receipt),
            },
            line => line,
        }
    }
}

impl Fill for serde_json::Value {}

pub(crate) fn records<T: Fill>(
    path: &Path,
    from: u64,
) -> impl Iterator<Item = io::Result<T>> + use<T> {
    let mut reader = open(path, from).map(Some);
    let mut strict = from == 0;
    let mut line = Vec::new();
    std::iter::from_fn(move || {
        loop {
            let file = match &mut reader {
                Ok(Some(file)) => file,
                Ok(None) => return None,
                Err(_) => return std::mem::replace(&mut reader, Ok(None)).err().map(Err),
            };
            line.clear();
            let error = match file.read_until(b'\n', &mut line) {
                Ok(0) => None,
                Ok(_) if line.trim_ascii().is_empty() => continue,
                Ok(_) => {
                    let whole = line.last() == Some(&b'\n');
                    let first = std::mem::take(&mut strict) && whole;
                    if whole && std::str::from_utf8(&line).is_err() {
                        Some(invalid("a log line is not valid UTF-8"))
                    } else {
                        match serde_json::from_slice::<T>(&line) {
                            Ok(record) => return Some(Ok(record.fill())),
                            Err(error) if first => return Some(Err(error.into())),
                            Err(_) => continue,
                        }
                    }
                }
                Err(error) => Some(error),
            };
            reader = Ok(None);
            return error.map(Err);
        }
    })
}

fn open(path: &Path, from: u64) -> io::Result<BufReader<File>> {
    let mut file = File::open(path)?;
    if from > 0 {
        file.seek(SeekFrom::Start(from - 1))?;
        let mut byte = [0_u8];
        if file.read(&mut byte)? != 1 || byte != *b"\n" {
            return Err(invalid("the cached length is not a line boundary"));
        }
    }
    Ok(BufReader::new(file))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
