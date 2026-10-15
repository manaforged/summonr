use std::fs::Metadata;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

use super::listing;

pub(crate) fn own_files(root: &Path) -> Vec<String> {
    let own = own_outputs();
    if own.is_empty() {
        return Vec::new();
    }
    listing(root)
        .unwrap_or_default()
        .into_iter()
        .filter(|relative| {
            let path = root.join(relative);
            path.symlink_metadata()
                .is_ok_and(|metadata| is_own(&path, &metadata, &own))
        })
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .collect()
}

#[cfg(unix)]
pub(super) type Own = (u64, u64);

#[cfg(windows)]
pub(super) type Own = PathBuf;

#[cfg(unix)]
pub(super) fn identity(metadata: &Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(unix)]
pub(super) fn is_own(_: &Path, metadata: &Metadata, own: &[Own]) -> bool {
    identity(metadata).is_some_and(|id| own.contains(&id))
}

#[cfg(windows)]
pub(super) fn is_own(path: &Path, _: &Metadata, own: &[Own]) -> bool {
    own.iter().any(|mine| mine.file_name() == path.file_name())
        && path.canonicalize().is_ok_and(|real| {
            own.iter()
                .any(|mine| mine.as_os_str().eq_ignore_ascii_case(real.as_os_str()))
        })
}

#[cfg(unix)]
pub(super) fn own_outputs() -> Vec<Own> {
    use std::os::fd::AsFd;
    [
        std::io::stdout().as_fd().try_clone_to_owned(),
        std::io::stderr().as_fd().try_clone_to_owned(),
    ]
    .into_iter()
    .flatten()
    .filter_map(|fd| std::fs::File::from(fd).metadata().ok())
    .filter(Metadata::is_file)
    .filter_map(|metadata| identity(&metadata))
    .collect()
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "Windows has no safe API for the path behind a standard handle"
)]
pub(super) fn own_outputs() -> Vec<Own> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW,
    };
    [
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ]
    .into_iter()
    .filter_map(|handle| {
        let mut buffer = vec![0_u16; 1024];
        // SAFETY: the handle is this process's own standard stream, and buffer is writable for its full length.
        let len = unsafe {
            GetFinalPathNameByHandleW(
                handle,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                FILE_NAME_NORMALIZED,
            )
        } as usize;
        (len > 0 && len < buffer.len())
            .then(|| PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len])))
    })
    .collect()
}
