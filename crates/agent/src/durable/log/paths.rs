use std::fs::{self, File};
use std::path::{Path, PathBuf};

use super::{DIR, FILE, SessionId, WORK};

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
