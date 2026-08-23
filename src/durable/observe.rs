//! Reference workspace observer.
//!
//! This module owns the safe bridge from a real local directory to a format-2
//! [`WorkspaceObservation`]. It walks the tree without following symlinks,
//! classifies every relative path against the recorded scope, hashes regular
//! file bytes and symlink targets with domain-tagged digests, and enforces the
//! entry and payload bounds of the candidate contract. It never returns a
//! partial observation: an unsupported kind, unreadable entry, identity
//! mismatch, or bound overrun fails the whole walk.
//!
//! [`workspace_identity_of`] is the single place that derives a
//! [`WorkspaceIdentity`] from a real root, so the workspace I/O owner and the
//! observer agree on identity.

use std::ffi::OsStr;
use std::path::{Component, Path};

use thiserror::Error;

use super::bound::{Bytes, Items};
use super::id::{Digest, ObservationId};
use super::workspace::{
    NativeRoot, ObservedEntry, ObservedKind, PathComponent, PathKey, PathPlatform,
    ReservedCoverage, WorkspaceError, WorkspaceIdentity, WorkspaceObservation, WorkspaceScope,
};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt as _;
#[cfg(not(unix))]
use std::os::windows::ffi::OsStrExt as _;

#[cfg(unix)]
fn os_str_bytes(value: &OsStr) -> Vec<u8> {
    value.as_bytes().to_vec()
}

#[cfg(unix)]
fn os_str_units(value: &OsStr) -> Vec<u16> {
    String::from_utf8_lossy(value.as_bytes())
        .encode_utf16()
        .collect()
}

#[cfg(not(unix))]
fn os_str_bytes(value: &OsStr) -> Vec<u8> {
    value.to_string_lossy().into_owned().into_bytes()
}

#[cfg(not(unix))]
fn os_str_units(value: &OsStr) -> Vec<u16> {
    value.encode_wide().collect()
}

const MAX_ENTRIES: usize = 100_000;
const MAX_PAYLOAD_BYTES: usize = 1_048_576;

#[derive(Debug, Error)]
pub(crate) enum ObserveError {
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("workspace root is not a directory")]
    RootNotDirectory,
    #[error("workspace root identity does not match the recorded scope")]
    IdentityMismatch,
    #[error("path escapes the workspace root: {0}")]
    Escape(String),
    #[error("unsupported filesystem entry kind")]
    UnsupportedKind,
    #[error("entry is unreadable: {0}")]
    Unreadable(String),
    #[error("file changed while it was read")]
    ChangingTree,
    #[error("observation exceeds a configured bound")]
    Bound,
}

/// Derive the workspace identity of a real directory.
pub(crate) fn workspace_identity_of(path: &Path) -> Result<WorkspaceIdentity, ObserveError> {
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        return Err(ObserveError::RootNotDirectory);
    }
    let (root, components) = native_root_and_components(&canonical)?;
    let object = workspace_object_digest(&canonical)?;
    WorkspaceIdentity::new(root, components, object).map_err(ObserveError::Workspace)
}

/// Observe a real workspace root against a recorded scope.
pub(crate) fn observe(
    path: &Path,
    scope: &WorkspaceScope,
    id: ObservationId,
) -> Result<WorkspaceObservation, ObserveError> {
    if !path.is_dir() {
        return Err(ObserveError::RootNotDirectory);
    }
    if scope.workspace().object() != workspace_object_digest(path)? {
        return Err(ObserveError::IdentityMismatch);
    }
    let platform = scope.platform();
    let mut entries = Vec::new();
    walk(path, path, scope, &mut entries)?;
    entries.sort_by(|left, right| left.path().cmp(right.path()));
    WorkspaceObservation::new(id, scope.id(), platform, entries).map_err(ObserveError::Workspace)
}

fn walk(
    root: &Path,
    directory: &Path,
    scope: &WorkspaceScope,
    entries: &mut Vec<ObservedEntry>,
) -> Result<(), ObserveError> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let real = entry.path();
        let file_type = entry.file_type()?;
        let key = relative_key(root, &real)?;
        match classify(&key, scope) {
            Some(Class::Candidate) | Some(Class::ReservedEntry) => {
                if entries.len() >= MAX_ENTRIES {
                    return Err(ObserveError::Bound);
                }
                entries.push(ObservedEntry::new(
                    key.clone(),
                    observe_kind(&real, &file_type)?,
                ));
            }
            Some(Class::Excluded) | None => {}
        }
        if file_type.is_dir() {
            walk(root, &real, scope, entries)?;
        }
    }
    Ok(())
}

fn classify(key: &PathKey, scope: &WorkspaceScope) -> Option<Class> {
    for artifact in scope.artifacts() {
        if artifact.path.prefixes(key) {
            return Some(Class::Excluded);
        }
    }
    for reserved in scope.reserved() {
        match reserved.coverage() {
            ReservedCoverage::Entry if reserved.path() == key => {
                return Some(Class::ReservedEntry);
            }
            ReservedCoverage::Subtree if reserved.path().prefixes(key) => {
                return Some(Class::Excluded);
            }
            _ => {}
        }
    }
    scope
        .candidate_roots()
        .iter()
        .any(|root| root.prefixes(key))
        .then_some(Class::Candidate)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Candidate,
    ReservedEntry,
    Excluded,
}

fn observe_kind(real: &Path, file_type: &std::fs::FileType) -> Result<ObservedKind, ObserveError> {
    if file_type.is_symlink() {
        let target = std::fs::read_link(real)?;
        let units = target_bytes(&target);
        let digest = Digest::hash(b"mage:workspace-symlink:v1", &units);
        return Ok(ObservedKind::Symlink { target: digest });
    }
    if file_type.is_dir() {
        return Ok(ObservedKind::Directory);
    }
    if file_type.is_file() {
        let before = metadata(real)?;
        let bytes =
            std::fs::read(real).map_err(|error| ObserveError::Unreadable(error.to_string()))?;
        if bytes.len() > MAX_PAYLOAD_BYTES {
            return Err(ObserveError::Bound);
        }
        let after = metadata(real)?;
        if !same_file(&before, &after) {
            return Err(ObserveError::ChangingTree);
        }
        let content = Digest::hash(b"mage:workspace-file:v1", &bytes);
        return Ok(ObservedKind::File {
            content,
            executable: is_executable(&before),
        });
    }
    Err(ObserveError::UnsupportedKind)
}

#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn metadata(path: &Path) -> std::io::Result<std::fs::Metadata> {
    std::fs::symlink_metadata(path)
}

#[cfg(not(unix))]
fn metadata(path: &Path) -> std::io::Result<std::fs::Metadata> {
    std::fs::symlink_metadata(path)
}

#[cfg(unix)]
fn same_file(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    before.dev() == after.dev() && before.ino() == after.ino() && before.len() == after.len()
}

#[cfg(not(unix))]
fn same_file(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    before.len() == after.len()
}

fn workspace_object_digest(path: &Path) -> Result<Digest, ObserveError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = std::fs::symlink_metadata(path)?;
        let mut body = Vec::new();
        body.extend_from_slice(&metadata.dev().to_be_bytes());
        body.extend_from_slice(&metadata.ino().to_be_bytes());
        Ok(Digest::hash(b"mage:workspace-object:v1", &body))
    }
    #[cfg(not(unix))]
    {
        use std::os::windows::fs::MetadataExt as _;
        let metadata = std::fs::symlink_metadata(path)?;
        let body = metadata.file_index().to_be_bytes();
        Ok(Digest::hash(b"mage:workspace-object:v1", &body))
    }
}

fn native_root_and_components(
    canonical: &Path,
) -> Result<(NativeRoot, Vec<PathComponent>), ObserveError> {
    let platform = platform();
    let mut components = Vec::new();
    for part in canonical.components() {
        if let Component::Normal(value) = part {
            let component = match platform {
                PathPlatform::Unix => PathComponent::unix(os_str_bytes(value)),
                PathPlatform::Windows => PathComponent::windows(os_str_units(value)),
            };
            components.push(component.map_err(ObserveError::Workspace)?);
        }
    }
    let native_root = match platform {
        PathPlatform::Unix => NativeRoot::Unix(
            Bytes::new(b"/".to_vec())
                .map_err(|error| ObserveError::Workspace(WorkspaceError::Bound(error)))?,
        ),
        PathPlatform::Windows => {
            let units = normal_components(canonical)
                .into_iter()
                .next()
                .map(os_str_units)
                .unwrap_or_default();
            NativeRoot::Windows(
                Items::new(units)
                    .map_err(|error| ObserveError::Workspace(WorkspaceError::Bound(error)))?,
            )
        }
    };
    Ok((native_root, components))
}

fn normal_components(canonical: &Path) -> Vec<&OsStr> {
    canonical
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value),
            _ => None,
        })
        .collect()
}

fn target_bytes(target: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        os_str_bytes(target.as_os_str())
    }
    #[cfg(not(unix))]
    {
        os_str_units(target.as_os_str())
            .into_iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect()
    }
}

fn relative_key(root: &Path, real: &Path) -> Result<PathKey, ObserveError> {
    let relative = real
        .strip_prefix(root)
        .map_err(|error| ObserveError::Escape(error.to_string()))?;
    let platform = platform();
    let mut components = Vec::new();
    for part in relative.components() {
        if let Component::Normal(value) = part {
            let component = match platform {
                PathPlatform::Unix => PathComponent::unix(os_str_bytes(value))?,
                PathPlatform::Windows => PathComponent::windows(os_str_units(value))?,
            };
            components.push(component);
        }
    }
    PathKey::new(platform, components).map_err(ObserveError::Workspace)
}

fn platform() -> PathPlatform {
    #[cfg(unix)]
    {
        PathPlatform::Unix
    }
    #[cfg(not(unix))]
    {
        PathPlatform::Windows
    }
}
