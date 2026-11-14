#![cfg(unix)]
#![allow(
    dead_code,
    reason = "each test target links fixtures but not all read state"
)]

use std::path::{Path, PathBuf};

pub(crate) fn link(name: &str, at: &Path) -> PathBuf {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::os::unix::fs::symlink(fixture, at).unwrap();
    at.to_path_buf()
}

pub(crate) fn state(link: &Path, name: &str) -> PathBuf {
    let mut path = link.as_os_str().to_owned();
    path.push(".");
    path.push(name);
    PathBuf::from(path)
}
