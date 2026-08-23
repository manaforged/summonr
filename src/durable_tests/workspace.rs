use super::fixture;
use crate::durable::workspace::{
    ChangeEntry, ChangeKind, ChangeManifest, ObservedEntry, ObservedKind, PathPlatform,
    WorkspaceError, WorkspaceObservation,
};

#[test]
fn manifest_is_the_complete_difference_between_two_observations() {
    let a = fixture::unix_path(&[b"a"]);
    let b = fixture::unix_path(&[b"b"]);
    let c = fixture::unix_path(&[b"c"]);
    let d = fixture::unix_path(&[b"d"]);
    let old = fixture::digest("old");
    let new = fixture::digest("new");
    let target = fixture::digest("target");
    let base = WorkspaceObservation::new(
        fixture::id(1_100),
        fixture::scope().id(),
        PathPlatform::Unix,
        vec![
            ObservedEntry::new(
                a.clone(),
                ObservedKind::File {
                    content: old,
                    executable: false,
                },
            ),
            ObservedEntry::new(b.clone(), ObservedKind::Directory),
            ObservedEntry::new(c.clone(), ObservedKind::Symlink { target }),
        ],
    )
    .expect("base observation must be valid");
    let capture = WorkspaceObservation::new(
        fixture::id(1_101),
        fixture::scope().id(),
        PathPlatform::Unix,
        vec![
            ObservedEntry::new(
                a.clone(),
                ObservedKind::File {
                    content: new,
                    executable: false,
                },
            ),
            ObservedEntry::new(
                b.clone(),
                ObservedKind::File {
                    content: new,
                    executable: false,
                },
            ),
            ObservedEntry::new(d.clone(), ObservedKind::Directory),
        ],
    )
    .expect("capture observation must be valid");
    let expected = ChangeManifest::new(vec![
        ChangeEntry::new(
            a,
            ChangeKind::Modified,
            Some(ObservedKind::File {
                content: old,
                executable: false,
            }),
            Some(ObservedKind::File {
                content: new,
                executable: false,
            }),
        ),
        ChangeEntry::new(
            b,
            ChangeKind::KindChanged,
            Some(ObservedKind::Directory),
            Some(ObservedKind::File {
                content: new,
                executable: false,
            }),
        ),
        ChangeEntry::new(
            c,
            ChangeKind::Removed,
            Some(ObservedKind::Symlink { target }),
            None,
        ),
        ChangeEntry::new(d, ChangeKind::Added, None, Some(ObservedKind::Directory)),
    ])
    .expect("expected manifest must be valid");

    assert_eq!(base.changes(&capture), Ok(expected));
}

#[test]
fn manifest_rejects_observations_from_different_scopes() {
    let base = WorkspaceObservation::new(
        fixture::id(1_110),
        fixture::scope().id(),
        PathPlatform::Unix,
        Vec::new(),
    )
    .expect("base observation must be valid");
    let capture = WorkspaceObservation::new(
        fixture::id(1_111),
        fixture::id(1_112),
        PathPlatform::Unix,
        Vec::new(),
    )
    .expect("capture observation must be valid");

    assert_eq!(
        base.changes(&capture),
        Err(WorkspaceError::ObservationScope)
    );
}
