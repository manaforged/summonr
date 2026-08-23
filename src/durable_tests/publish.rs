use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::durable::bound::Items;
use crate::durable::fact::{MaterializationOutcome, PublicationFootprint};
use crate::durable::publish::{materialize, publish};
use crate::durable::workspace::{PathComponent, PathKey, PathPlatform};

use super::{fixture, work};

fn path(components: &[&[u8]]) -> PathKey {
    PathKey::new(
        PathPlatform::Unix,
        components
            .iter()
            .map(|component| PathComponent::unix(component.to_vec()).expect("component valid"))
            .collect(),
    )
    .expect("path must be valid")
}

fn footprint() -> PublicationFootprint {
    PublicationFootprint {
        ancestors: Items::new(vec![
            path(&[b".mage"]),
            path(&[b".mage", b"plans"]),
            path(&[b".mage", b"plans", b"sess"]),
            path(&[b".mage", b"plans", b"sess", b"work"]),
        ])
        .expect("ancestors fit"),
        temporary: path(&[b".mage", b"plans", b"sess", b"work", b".mage-attempt.tmp"]),
        destination: path(&[b".mage", b"plans", b"sess", b"work", b"rev.md"]),
    }
}

fn dest_dir(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".mage/plans/sess/work")
}

#[test]
fn publish_writes_a_fresh_destination_and_creates_its_ancestors() {
    let dir = tempfile::tempdir().expect("temporary directory must exist");
    let root = dir.path();
    let bytes = b"# Plan\n- step\n";

    let outcome = publish(root, &footprint(), bytes);

    assert!(matches!(outcome, MaterializationOutcome::Written));
    let written = dest_dir(root).join("rev.md");
    assert_eq!(
        std::fs::read(&written).expect("destination must exist"),
        bytes
    );
}

#[test]
fn publish_matches_an_existing_identical_destination() {
    let dir = tempfile::tempdir().expect("temporary directory must exist");
    let root = dir.path();
    let bytes = b"# Plan\n- step\n";
    let directory = dest_dir(root);
    std::fs::create_dir_all(&directory).expect("destination dir must exist");
    std::fs::write(directory.join("rev.md"), bytes).expect("destination must be written");

    let outcome = publish(root, &footprint(), bytes);

    assert!(matches!(outcome, MaterializationOutcome::Matched));
}

#[test]
fn publish_conflicts_with_divergent_bytes() {
    let dir = tempfile::tempdir().expect("temporary directory must exist");
    let root = dir.path();
    let bytes = b"# Plan\n- step\n";
    let directory = dest_dir(root);
    std::fs::create_dir_all(&directory).expect("destination dir must exist");
    std::fs::write(directory.join("rev.md"), b"# divergent").expect("destination must be written");

    let outcome = publish(root, &footprint(), bytes);

    assert!(matches!(outcome, MaterializationOutcome::Conflict { .. }));
    // The existing divergent bytes must not be replaced.
    assert_eq!(
        std::fs::read(directory.join("rev.md")).expect("destination must be intact"),
        b"# divergent"
    );
}

fn real(root: &Path, key: &PathKey) -> PathBuf {
    let mut path = root.to_path_buf();
    for component in key.components() {
        match component {
            PathComponent::Unix(bytes) => path.push(OsStr::new(
                std::str::from_utf8(bytes.as_slice()).expect("path must be UTF-8 in this test"),
            )),
            PathComponent::Windows(_) => unreachable!("Unix-only test"),
        }
    }
    path
}

#[test]
fn materialize_converges_when_recovery_reruns_after_a_crash() {
    // The first run performs the FS write (the crash point is before the
    // durable terminal fact). A crash leaves the destination published.
    // Recovery re-runs the same footprint; it must converge to a usable
    // outcome and never rewrite or diverge.
    let dir = tempfile::tempdir().expect("temporary directory must exist");
    let root = dir.path();
    let plan = work::plan_revision();
    let attempt = fixture::id(901);

    let first = materialize(root, PathPlatform::Unix, fixture::session(), &plan, attempt);
    assert!(
        first.usable(),
        "first run must converge to written or matched"
    );

    let second = materialize(root, PathPlatform::Unix, fixture::session(), &plan, attempt);
    assert!(second.usable(), "recovery must converge to a usable result");
    assert!(matches!(second, MaterializationOutcome::Matched));
}

#[test]
fn materialize_writes_the_plan_markdown_at_its_qualified_path() {
    let dir = tempfile::tempdir().expect("temporary directory must exist");
    let root = dir.path();
    let plan = work::plan_revision();

    let outcome = materialize(
        root,
        PathPlatform::Unix,
        fixture::session(),
        &plan,
        fixture::id(900),
    );

    assert!(matches!(outcome, MaterializationOutcome::Written));
    let destination = crate::durable::work::plan_path(
        PathPlatform::Unix,
        fixture::session(),
        plan.work(),
        plan.id(),
    )
    .expect("Plan path must be valid");
    assert_eq!(
        std::fs::read(real(root, &destination)).expect("Plan file must exist"),
        plan.markdown().as_bytes()
    );
}

#[cfg(unix)]
#[test]
fn publish_fails_when_an_ancestor_is_a_symlink() {
    let dir = tempfile::tempdir().expect("temporary directory must exist");
    let root = dir.path();
    let elsewhere = tempfile::tempdir().expect("elsewhere must exist");
    // Replace the `.mage` ancestor with a symlink that would redirect a
    // checked-path operation outside the workspace.
    std::os::unix::fs::symlink(elsewhere.path(), root.join(".mage"))
        .expect("symlink must be created");

    let outcome = publish(root, &footprint(), b"# Plan");
    assert!(matches!(outcome, MaterializationOutcome::Failed { .. }));
}
