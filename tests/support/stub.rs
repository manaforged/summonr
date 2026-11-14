#![cfg(unix)]

use std::path::{Path, PathBuf};

pub(crate) fn state(link: &Path, name: &str) -> PathBuf {
    let mut path = link.as_os_str().to_owned();
    path.push(".");
    path.push(name);
    PathBuf::from(path)
}
