#[cfg(unix)]
use super::*;

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
#[test]
fn a_nested_symlink_cannot_redirect_publication() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let elsewhere = tempfile::tempdir().expect("elsewhere must exist");
    std::fs::create_dir(directory.path().join(".mage")).expect("first ancestor must exist");
    std::os::unix::fs::symlink(elsewhere.path(), directory.path().join(".mage/plans"))
        .expect("deeper symlink must be created");

    let outcome = publish(directory.path(), &footprint(), b"# Plan");

    assert!(matches!(outcome, MaterializationOutcome::Failed { .. }));
    assert!(!elsewhere.path().join("sess/work/rev.md").exists());
}
