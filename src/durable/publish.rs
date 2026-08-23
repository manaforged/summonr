//! Local create-only publication.
//!
//! This module materializes a Plan projection at its exact publication
//! footprint. It opens each ancestor relative to the previous directory
//! capability without following symlinks, creates a same-directory temporary
//! file exclusively, writes and reads back the bytes, and publishes the
//! destination name with a directory-relative hard link that never replaces an
//! existing owner. Any change to a checked ancestor or a racing destination
//! yields a typed result instead of an ambient-path overwrite.
//!
//! The POSIX adapter uses [`rustix`] directory-relative `openat` / `linkat`.
//! The non-POSIX adapter returns a visible unsupported result; it never falls
//! back to an ambient path operation.

use std::io::{Read, Write};
use std::path::Path;

use rustix::fs::{AtFlags, Mode, OFlags, linkat, mkdirat, open, openat, unlinkat};

use super::bound::{Items, Text};
use super::fact::{MaterializationOutcome, PublicationFootprint};
use super::id::{Digest, MaterializationId, PlanRevisionId, SessionId, WorkId};
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
    #[cfg(unix)]
    {
        publish_unix(root, footprint, bytes)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, footprint, bytes);
        failed("create-only publication is unsupported on this platform")
    }
}

#[cfg(unix)]
fn publish_unix(
    root: &Path,
    footprint: &PublicationFootprint,
    bytes: &[u8],
) -> MaterializationOutcome {
    let mut cap = match open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
        Mode::from_bits_retain(0),
    ) {
        Ok(fd) => fd,
        Err(error) => return failed(format!("cannot open workspace root: {error}")),
    };
    for ancestor in footprint.ancestors.iter() {
        let name = component_bytes(ancestor.last().expect("ancestor has a name"));
        match openat(
            &cap,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
            Mode::from_bits_retain(0),
        ) {
            Ok(next) => cap = next,
            Err(rustix::io::Errno::NOENT) => {
                if let Err(error) = mkdirat(&cap, name, Mode::from_bits_retain(0o755)) {
                    return failed(format!("cannot create ancestor directory: {error}"));
                }
                match openat(
                    &cap,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
                    Mode::from_bits_retain(0),
                ) {
                    Ok(next) => cap = next,
                    Err(error) => return failed(format!("cannot open created directory: {error}")),
                }
            }
            Err(error) => return failed(format!("cannot open ancestor directory: {error}")),
        }
    }
    let temporary = component_bytes(footprint.temporary.last().expect("temporary has a name"));
    let destination = component_bytes(
        footprint
            .destination
            .last()
            .expect("destination has a name"),
    );

    let temp_fd = match openat(
        &cap,
        temporary,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
        Mode::from_bits_retain(0o644),
    ) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::EXIST) => {
            // A leftover same-attempt temporary is derived state. Best-effort
            // cleanup owns no state; recreate it exclusively.
            ignore(unlinkat(&cap, temporary, AtFlags::empty()));
            match openat(
                &cap,
                temporary,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
                Mode::from_bits_retain(0o644),
            ) {
                Ok(fd) => fd,
                Err(error) => return failed(format!("cannot create temporary file: {error}")),
            }
        }
        Err(error) => return failed(format!("cannot create temporary file: {error}")),
    };

    let mut file = std::fs::File::from(temp_fd);
    let write = file
        .write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string());
    drop(file);
    if let Err(error) = write {
        ignore(unlinkat(&cap, temporary, AtFlags::empty()));
        return failed(format!("cannot write temporary file: {error}"));
    }
    if !matches!(compare(&cap, temporary, bytes), TempStates::Match) {
        ignore(unlinkat(&cap, temporary, AtFlags::empty()));
        return failed("temporary bytes do not match the recorded Plan");
    }

    match linkat(&cap, temporary, &cap, destination, AtFlags::empty()) {
        Ok(()) => {
            ignore(unlinkat(&cap, temporary, AtFlags::empty()));
            classify_destination(&cap, destination, bytes, true)
        }
        Err(rustix::io::Errno::EXIST) => {
            ignore(unlinkat(&cap, temporary, AtFlags::empty()));
            classify_destination(&cap, destination, bytes, false)
        }
        Err(error) => {
            ignore(unlinkat(&cap, temporary, AtFlags::empty()));
            failed(format!("cannot publish destination: {error}"))
        }
    }
}

fn classify_destination(
    cap: &rustix::fd::OwnedFd,
    name: &[u8],
    expected: &[u8],
    published: bool,
) -> MaterializationOutcome {
    let states = compare(cap, name, expected);
    match states {
        TempStates::Match => {
            if published {
                MaterializationOutcome::Written
            } else {
                MaterializationOutcome::Matched
            }
        }
        TempStates::Conflict { observed } => MaterializationOutcome::Conflict { observed },
        TempStates::ReadError | TempStates::Gone => {
            failed("cannot classify destination after publication")
        }
    }
}

fn compare(cap: &rustix::fd::OwnedFd, name: &[u8], expected: &[u8]) -> TempStates {
    let fd = match openat(
        cap,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW,
        Mode::from_bits_retain(0),
    ) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => return TempStates::Gone,
        Err(_) => return TempStates::ReadError,
    };
    let bytes = read_all(fd);
    if bytes == expected {
        TempStates::Match
    } else {
        TempStates::Conflict {
            observed: Digest::hash(b"mage:plan:v1", &bytes),
        }
    }
}

fn read_all(fd: rustix::fd::OwnedFd) -> Vec<u8> {
    let mut file = std::fs::File::from(fd);
    let mut buf = Vec::new();
    ignore(file.read_to_end(&mut buf));
    buf
}

/// Consume an ignored fallible result. Best-effort cleanup owns no state.
fn ignore<T>(_: T) {}

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

enum TempStates {
    Match,
    Conflict { observed: Digest },
    Gone,
    ReadError,
}

#[cfg(unix)]
fn component_bytes(component: &PathComponent) -> &[u8] {
    match component {
        PathComponent::Unix(bytes) => bytes.as_slice(),
        PathComponent::Windows(_) => unreachable!("Unix path has no Windows component"),
    }
}
