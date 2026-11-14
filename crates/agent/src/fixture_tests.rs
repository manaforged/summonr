use std::path::{Path, PathBuf};

pub(crate) fn path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

pub(crate) fn link(name: &str, at: &Path) -> PathBuf {
    std::os::unix::fs::symlink(path(name), at).unwrap();
    at.to_path_buf()
}
