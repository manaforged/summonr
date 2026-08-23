use std::path::Path;

use crate::durable::observe::{ObserveError, observe, workspace_identity_of};
use crate::durable::workspace::{
    ArtifactRoot, ObservedKind, PathComponent, PathKey, PathPlatform, ReservedCoverage,
    ReservedPath, WorkspaceScope,
};

use super::fixture;

fn scope(
    workspace: crate::durable::workspace::WorkspaceIdentity,
    reserved: Vec<ReservedPath>,
    artifacts: Vec<ArtifactRoot>,
) -> WorkspaceScope {
    WorkspaceScope::new(
        fixture::id(10),
        workspace,
        vec![PathKey::root(PathPlatform::Unix)],
        reserved,
        artifacts,
    )
    .expect("test scope must be valid")
}

fn identity(root: &Path) -> crate::durable::workspace::WorkspaceIdentity {
    workspace_identity_of(root).expect("workspace root identity must resolve")
}

fn reserved(path: &[&[u8]], coverage: ReservedCoverage) -> ReservedPath {
    let components = path
        .iter()
        .map(|component| PathComponent::unix(component.to_vec()).expect("component must be valid"))
        .collect();
    ReservedPath::new(
        PathKey::new(PathPlatform::Unix, components).expect("reserved path must be valid"),
        coverage,
    )
}

fn artifact(path: &[&[u8]]) -> ArtifactRoot {
    let components = path
        .iter()
        .map(|component| PathComponent::unix(component.to_vec()).expect("component must be valid"))
        .collect();
    ArtifactRoot {
        path: PathKey::new(PathPlatform::Unix, components).expect("artifact path must be valid"),
        rule: fixture::id(1),
    }
}

fn setup() -> (tempfile::TempDir, WorkspaceScope) {
    let dir = tempfile::tempdir().expect("temporary directory must exist");
    let scope = scope(identity(dir.path()), Vec::new(), Vec::new());
    (dir, scope)
}

#[test]
fn default_candidate_root_includes_files_and_empty_directories() {
    let (dir, scope) = setup();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), b"hello").expect("file must be written");
    std::fs::create_dir(root.join("empty-dir")).expect("empty directory must be created");

    let observation = observe(root, &scope, fixture::id(20)).expect("observation must complete");

    let entries = observation.entries;
    let file = entries
        .iter()
        .find(|entry| {
            entry
                .path()
                .last()
                .is_some_and(|last| last == &PathComponent::unix(b"a.txt".to_vec()).unwrap())
        })
        .expect("a.txt must be observed");
    assert!(matches!(
        file.kind(),
        ObservedKind::File {
            executable: false,
            ..
        }
    ));

    let directory = entries
        .iter()
        .find(|entry| {
            entry
                .path()
                .last()
                .is_some_and(|last| last == &PathComponent::unix(b"empty-dir".to_vec()).unwrap())
        })
        .expect("empty directory must be observed");
    assert!(matches!(directory.kind(), ObservedKind::Directory));
}

#[test]
fn reserved_subtree_excludes_descendants() {
    let dir = tempfile::tempdir().expect("temporary directory must exist");
    let root = dir.path();
    std::fs::create_dir_all(root.join(".mage/plans")).expect(".mage/plans must be created");
    std::fs::write(root.join(".mage/plans/plan.md"), b"plan").expect("plan must be written");
    std::fs::write(root.join(".mage/notes.txt"), b"notes").expect("notes must be written");
    let scope = scope(
        identity(root),
        vec![reserved(&[b".mage", b"plans"], ReservedCoverage::Subtree)],
        Vec::new(),
    );

    let observation = observe(root, &scope, fixture::id(30)).expect("observation must complete");

    let paths = observation
        .entries
        .iter()
        .map(path_bytes)
        .collect::<Vec<Vec<&[u8]>>>();
    assert!(
        paths
            .iter()
            .any(|path| path == &[b".mage" as &[u8], b"notes.txt"])
    );
    assert!(
        !paths
            .iter()
            .any(|path| path == &[b".mage" as &[u8], b"plans"])
    );
}

#[test]
fn reserved_entry_excludes_only_the_named_entry() {
    let dir = tempfile::tempdir().expect("temporary directory must exist");
    let root = dir.path();
    std::fs::create_dir_all(root.join(".mage/other")).expect(".mage/other must be created");
    std::fs::write(root.join(".mage/other/file.txt"), b"file").expect("file must be written");
    let scope = scope(
        identity(root),
        vec![reserved(&[b".mage"], ReservedCoverage::Entry)],
        Vec::new(),
    );

    let observation = observe(root, &scope, fixture::id(31)).expect("observation must complete");

    let paths = observation
        .entries
        .iter()
        .map(path_bytes)
        .collect::<Vec<Vec<&[u8]>>>();
    // An Entry reservation is observed (so a tool write is later visible) but
    // it does not exclude an unrelated descendant.
    assert!(paths.iter().any(|path| path == &[b".mage" as &[u8]]));
    assert!(
        paths
            .iter()
            .any(|path| path == &[b".mage" as &[u8], b"other", b"file.txt"])
    );
}

#[test]
fn artifact_root_is_excluded_from_candidate_entries() {
    let dir = tempfile::tempdir().expect("temporary directory must exist");
    let root = dir.path();
    std::fs::create_dir_all(root.join("node_modules/pkg")).expect("node_modules must be created");
    std::fs::write(root.join("node_modules/pkg/index.js"), b"// js").expect("file must be written");
    let scope = scope(
        identity(root),
        Vec::new(),
        vec![artifact(&[b"node_modules"])],
    );

    let observation = observe(root, &scope, fixture::id(40)).expect("observation must complete");

    assert!(observation.entries.is_empty());
}

#[test]
fn a_different_root_object_is_rejected() {
    let first = tempfile::tempdir().expect("first temporary directory must exist");
    let second = tempfile::tempdir().expect("second temporary directory must exist");
    let scope = scope(identity(first.path()), Vec::new(), Vec::new());

    let result = observe(second.path(), &scope, fixture::id(50));
    assert!(matches!(result, Err(ObserveError::IdentityMismatch)));
}

#[test]
fn executable_file_records_the_executable_bit() {
    let (dir, scope) = setup();
    let root = dir.path();
    let script = root.join("run.sh");
    std::fs::write(&script, b"#!/bin/sh\n").expect("script must be written");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = std::fs::metadata(&script)
            .expect("script metadata must exist")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).expect("permissions must be set");
    }

    let observation = observe(root, &scope, fixture::id(60)).expect("observation must complete");
    let script_entry = observation
        .entries
        .iter()
        .find(|entry| {
            entry
                .path()
                .last()
                .is_some_and(|last| last == &PathComponent::unix(b"run.sh".to_vec()).unwrap())
        })
        .expect("run.sh must be observed");
    assert!(matches!(
        script_entry.kind(),
        ObservedKind::File {
            executable: true,
            ..
        }
    ));
}

#[test]
fn observe_never_follows_a_symlink_target_outside_the_workspace() {
    let (dir, scope) = setup();
    let root = dir.path();
    let outside = tempfile::tempdir().expect("outside directory must exist");
    std::fs::write(outside.path().join("secret"), b"secret").expect("secret must be written");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), root.join("external"))
        .expect("symlink must be created");

    let observation = observe(root, &scope, fixture::id(80)).expect("observation must complete");
    #[cfg(unix)]
    {
        let external = observation
            .entries
            .iter()
            .find(|entry| {
                entry
                    .path()
                    .last()
                    .is_some_and(|last| last == &PathComponent::unix(b"external".to_vec()).unwrap())
            })
            .expect("external symlink must be observed");
        assert!(matches!(external.kind(), ObservedKind::Symlink { .. }));
    }
}

#[cfg(unix)]
#[test]
fn a_fifo_is_an_unsupported_kind() {
    let dir = tempfile::tempdir().expect("temporary directory must exist");
    let root = dir.path();
    let fifo = root.join("pipe");
    use std::process::Command;
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo must run")
            .success()
    );
    let scope = scope(identity(root), Vec::new(), Vec::new());

    let result = observe(root, &scope, fixture::id(70));
    assert!(matches!(result, Err(ObserveError::UnsupportedKind)));
}

fn path_bytes(entry: &crate::durable::workspace::ObservedEntry) -> Vec<&[u8]> {
    entry
        .path()
        .components()
        .iter()
        .map(|component| match component {
            PathComponent::Unix(value) => value.as_slice(),
            PathComponent::Windows(_) => unreachable!("Unix-only test"),
        })
        .collect()
}
