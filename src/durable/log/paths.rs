use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use super::{DIR, FILE, Line, SessionId, WORK};

pub(crate) fn read(path: &Path) -> io::Result<Vec<Line>> {
    Ok(records(path)?.into_iter().map(receipt_text).collect())
}

fn receipt_text(line: Line) -> Line {
    match line {
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

pub(crate) fn whole_len(path: &Path) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok().filter(fs::Metadata::is_file)?.len();
    if len == 0 {
        return Some(0);
    }
    let mut last = [0_u8];
    file.seek(SeekFrom::End(-1)).ok()?;
    file.read_exact(&mut last).ok()?;
    (last == *b"\n").then_some(len)
}

pub(crate) fn records<T: DeserializeOwned>(path: &Path) -> io::Result<Vec<T>> {
    parse(&fs::read(path)?, true)
}

pub(crate) fn records_after<T: DeserializeOwned>(path: &Path, offset: u64) -> io::Result<Vec<T>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = File::open(path)?;
    let start = offset.saturating_sub(1);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if offset > 0 && bytes.first() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the cached length is not a line boundary",
        ));
    }
    parse(
        bytes.get(usize::from(offset > 0)..).unwrap_or_default(),
        false,
    )
}

fn parse<T: DeserializeOwned>(bytes: &[u8], strict: bool) -> io::Result<Vec<T>> {
    let complete = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let text = std::str::from_utf8(&bytes[..complete])
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut lines = Vec::new();
    let mut first = strict;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(line) {
            Ok(line) => lines.push(line),
            Err(error) if first => return Err(error.into()),
            Err(_) => {}
        }
        first = false;
    }
    if let Ok(tail) = std::str::from_utf8(&bytes[complete..])
        && !tail.trim().is_empty()
        && let Ok(line) = serde_json::from_str(tail)
    {
        lines.push(line);
    }
    Ok(lines)
}

pub(crate) fn scan(path: &Path) -> io::Result<impl Iterator<Item = Line>> {
    use std::io::BufRead;
    let file = File::open(path)?;
    Ok(io::BufReader::new(file)
        .lines()
        .map_while(|line| line.ok())
        .filter_map(|line| serde_json::from_str(&line).ok())
        .map(receipt_text))
}

pub(crate) fn head(path: &Path, max: usize) -> io::Result<Vec<Line>> {
    use std::io::BufRead;
    let file = File::open(path)?;
    Ok(io::BufReader::new(file)
        .lines()
        .take(max)
        .map_while(|line| line.ok())
        .filter_map(|line| serde_json::from_str(&line).ok())
        .map(receipt_text)
        .collect())
}

pub(crate) fn header(path: &Path) -> io::Result<Line> {
    use std::io::BufRead;
    let mut first = String::new();
    io::BufReader::new(File::open(path)?).read_line(&mut first)?;
    Ok(serde_json::from_str(first.trim_end())?)
}

pub(crate) fn session_path(home: &Path, session: SessionId) -> PathBuf {
    home.join(DIR).join(session.to_string()).join(FILE)
}

pub(crate) fn work_path(session: &Path) -> PathBuf {
    session.with_file_name(WORK)
}

pub(crate) fn name_path(session: &Path) -> PathBuf {
    session.with_file_name("name")
}

pub(crate) fn sessions_dir(home: &Path) -> PathBuf {
    home.join(DIR)
}

pub(crate) fn parse_path(path: &Path) -> Option<SessionId> {
    if path.file_name()?.to_str()? != FILE {
        return None;
    }
    let dir = path.parent()?;
    if dir.parent()?.file_name()?.to_str()? != DIR {
        return None;
    }
    dir.file_name()?.to_str()?.parse().ok()
}

pub(crate) fn home_of(path: &Path) -> Option<&Path> {
    path.parent()?.parent()?.parent()
}
