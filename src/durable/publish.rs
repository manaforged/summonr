//! Local create-only publication.
//!
//! Materializes a Plan revision at its exact publication footprint: absent
//! ancestor directories are created, and the destination name is never
//! replaced — an existing identical destination is `matched`, a divergent one
//! is a typed `conflict`. Publication refuses a symlinked ancestor instead of
//! following it out of the workspace.

use std::path::{Path, PathBuf};

use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};

use super::bound::{Items, Text};
use super::fact::{MaterializationOutcome, PublicationFootprint};
use super::id::Digest;
use super::id::{MaterializationId, PlanRevisionId, SessionId, WorkId};
use super::work::{PlanRevision, plan_path};
use super::workspace::{PathComponent, PathKey, PathPlatform};

const MAX_DIAGNOSTIC_BYTES: usize = 65_536;

/// Publish `bytes` at the destination in `footprint`, relative to `root`.
///
/// Returns one of `written`, `matched`, `conflict`, or `failed`. Callers must
/// validate the footprint with [`PublicationFootprint::validate`] first; a
/// malformed footprint is a caller bug, not a classification outcome.
pub(crate) fn publish(
    root: &Path,
    footprint: &PublicationFootprint,
    bytes: &[u8],
) -> MaterializationOutcome {
    let root = match Dir::open_ambient_dir(root, ambient_authority()) {
        Ok(root) => root,
        Err(error) => return failed(format!("cannot open workspace: {error}")),
    };
    if let Some(name) = symlinked_ancestor(&root, footprint) {
        return failed(format!("ancestor `{name}` is a symlink"));
    }
    let temporary = relative(&footprint.temporary);
    let destination = relative(&footprint.destination);
    if let Some(parent) = destination.parent()
        && let Err(error) = root.create_dir_all(parent)
    {
        return failed(format!("cannot create ancestor directories: {error}"));
    }
    match root.read(&destination) {
        Ok(existing) if existing == bytes => MaterializationOutcome::Matched,
        Ok(existing) => MaterializationOutcome::Conflict {
            observed: Digest::hash(b"mage:plan:v1", &existing),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_fresh(&root, &temporary, &destination, bytes)
        }
        Err(error) => failed(format!("cannot inspect destination: {error}")),
    }
}

/// Write the temporary file and hard-link it into place. The link fails when
/// another writer claimed the destination first, which classifies from the
/// winner's bytes; the temporary never outlives the attempt.
fn write_fresh(
    root: &Dir,
    temporary: &Path,
    destination: &Path,
    bytes: &[u8],
) -> MaterializationOutcome {
    use std::io::Write;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let written = root.open_with(temporary, &options).or_else(|error| {
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error);
        }
        // A leftover temporary from a crashed same-attempt write is
        // derived state; recreate it exclusively.
        root.remove_file(temporary)?;
        root.open_with(temporary, &options)
    });
    let result = match &written {
        Ok(file) => {
            let mut file = file;
            file.write_all(bytes).and_then(|_| file.sync_all())
        }
        Err(error) => Err(std::io::Error::new(error.kind(), error.to_string())),
    };
    drop(written);
    if let Err(error) = result {
        drop(root.remove_file(temporary));
        return failed(format!("cannot write temporary file: {error}"));
    }
    match root.hard_link(temporary, root, destination) {
        Ok(()) => {
            drop(root.remove_file(temporary));
            MaterializationOutcome::Written
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            drop(root.remove_file(temporary));
            match root.read(destination) {
                Ok(existing) if existing == bytes => MaterializationOutcome::Matched,
                Ok(existing) => MaterializationOutcome::Conflict {
                    observed: Digest::hash(b"mage:plan:v1", &existing),
                },
                Err(error) => failed(format!("cannot classify destination: {error}")),
            }
        }
        Err(error) => {
            drop(root.remove_file(temporary));
            failed(format!("cannot publish destination: {error}"))
        }
    }
}

/// The first footprint ancestor that exists as a symlink, if any.
fn symlinked_ancestor(root: &Dir, footprint: &PublicationFootprint) -> Option<String> {
    for ancestor in footprint.ancestors.iter() {
        let walked = relative(ancestor);
        match root.symlink_metadata(&walked) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return walked
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned());
            }
            Ok(_) => {}
            // Absent ancestors are created by publication itself.
            Err(_) => return None,
        }
    }
    None
}

/// Materialize one Plan revision at its qualified path.
///
/// The footprint is the exact set of absent ancestor directories, the
/// attempt-temporary name, and the destination. A malformed footprint fails
/// before any write; every post-write crash converges to a typed outcome.
pub(crate) fn materialize(
    root: &Path,
    platform: PathPlatform,
    session: SessionId,
    plan: &PlanRevision,
    attempt: MaterializationId,
) -> MaterializationOutcome {
    let footprint = match footprint_for(platform, session, plan.work(), plan.id(), attempt) {
        Ok(footprint) => footprint,
        Err(error) => return failed(error),
    };
    if let Err(error) = footprint.validate(attempt.path_component().as_str()) {
        return failed(format!("invalid publication footprint: {error}"));
    }
    publish(root, &footprint, plan.markdown().as_bytes())
}

pub(crate) fn footprint_for(
    platform: PathPlatform,
    session: SessionId,
    work: WorkId,
    revision: PlanRevisionId,
    attempt: MaterializationId,
) -> Result<PublicationFootprint, String> {
    let destination = plan_path(platform, session, work, revision)
        .map_err(|error| format!("cannot derive Plan path: {error}"))?;
    let parent = destination
        .parent()
        .ok_or_else(|| "Plan path has no parent".to_string())?;
    let mut temporary = destination.components().to_vec();
    temporary.pop();
    let name = format!(".mage-{}.tmp", attempt.path_component());
    let component = match platform {
        PathPlatform::Unix => PathComponent::unix(name.into_bytes())
            .map_err(|error| format!("temporary name invalid: {error}"))?,
        PathPlatform::Windows => PathComponent::windows(name.encode_utf16().collect())
            .map_err(|error| format!("temporary name invalid: {error}"))?,
    };
    temporary.push(component);
    let temporary = PathKey::new(platform, temporary)
        .map_err(|error| format!("temporary path invalid: {error}"))?;
    let ancestors = ancestor_chain(&parent);
    Ok(PublicationFootprint {
        ancestors: Items::new(ancestors)
            .map_err(|error| format!("ancestor chain too long: {error}"))?,
        temporary,
        destination,
    })
}

fn ancestor_chain(parent: &PathKey) -> Vec<PathKey> {
    let mut chain = Vec::new();
    let mut current = Some(parent.clone());
    while let Some(ancestor) = current {
        chain.push(ancestor.clone());
        current = ancestor.parent();
    }
    chain.reverse();
    chain.retain(|ancestor| ancestor.len() > 0);
    chain
}

fn failed(diagnostic: impl Into<String>) -> MaterializationOutcome {
    let mut diagnostic = diagnostic.into();
    if diagnostic.len() > MAX_DIAGNOSTIC_BYTES {
        let mut cut = MAX_DIAGNOSTIC_BYTES;
        while !diagnostic.is_char_boundary(cut) {
            cut -= 1;
        }
        diagnostic.truncate(cut);
    }
    MaterializationOutcome::Failed {
        diagnostic: Text::new(diagnostic).expect("diagnostic now fits"),
    }
}

fn relative(key: &PathKey) -> PathBuf {
    let mut path = PathBuf::new();
    for component in key.components() {
        push_component(&mut path, component);
    }
    path
}

#[cfg(unix)]
fn push_component(path: &mut PathBuf, component: &PathComponent) {
    use std::os::unix::ffi::OsStrExt as _;
    if let PathComponent::Unix(bytes) = component {
        path.push(std::ffi::OsStr::from_bytes(bytes.as_slice()));
    }
}

#[cfg(test)]
#[path = "publish_tests.rs"]
mod tests;

#[cfg(not(unix))]
fn push_component(path: &mut PathBuf, component: &PathComponent) {
    if let PathComponent::Windows(units) = component {
        path.push(String::from_utf16_lossy(units.as_slice()));
    }
}
