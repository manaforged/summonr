use std::path::Path;
use std::str::FromStr;

use super::fixture;
use crate::durable::bound::Items;
use crate::durable::context::RequestHead;
use crate::durable::encoding::Commit;
use crate::durable::id::{Digest, SessionId, WorkId};
use crate::durable::journal::{parse_path, session_path};
use crate::durable::workspace::{
    ChangeEntry, ChangeKind, ChangeManifest, ObservedEntry, ObservedKind, PathComponent, PathKey,
    PathPlatform, WorkspaceObservation,
};

#[test]
fn identities_have_one_lowercase_portable_spelling() {
    let session = fixture::session();
    let spelling = session.path_component();
    assert_eq!(spelling, "00000000000000000000000000000001");
    assert_eq!(SessionId::from_str(&spelling), Ok(session));
    let alphabetic = SessionId::from_u128(0xab).path_component();
    SessionId::from_str(&alphabetic.to_uppercase()).expect_err("uppercase ID must fail");
    SessionId::from_str("text-session").expect_err("text display ID must fail");

    let work: WorkId = fixture::id(0xab);
    assert_eq!(work.path_component(), "000000000000000000000000000000ab");

    let home = Path::new("/tmp/mage-home");
    let path = session_path(home, SessionId::from_u128(0xab));
    assert_eq!(parse_path(&path), Some(SessionId::from_u128(0xab)));
    let uppercase = path.with_file_name("000000000000000000000000000000AB.jsonl");
    assert_eq!(parse_path(&uppercase), None);
}

#[test]
fn native_path_frames_preserve_unix_bytes_and_windows_wtf16() {
    let unix = PathKey::new(
        PathPlatform::Unix,
        vec![
            PathComponent::unix(vec![b'a', 0xff]).expect("Unix bytes are valid"),
            PathComponent::unix(b"b".to_vec()).expect("Unix bytes are valid"),
        ],
    )
    .expect("Unix path is valid");
    assert_eq!(
        unix.canonical(),
        vec![
            0, 1, 0, 0, 0, 0, 2, 0, 0, 0, 2, b'a', 0xff, 0, 0, 0, 1, b'b'
        ]
    );

    let windows = PathKey::new(
        PathPlatform::Windows,
        vec![PathComponent::windows(vec![0x0061, 0xd800, 0xdc00]).expect("WTF-16 units are valid")],
    )
    .expect("Windows path is valid");
    assert_eq!(
        windows.canonical(),
        vec![0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 6, 0, 0x61, 0xd8, 0, 0xdc, 0]
    );
}

#[test]
fn commit_request_and_observation_encodings_are_fixed() {
    let commit = Commit::new(1, fixture::id(1), None, None, fixture::root_facts())
        .expect("root commit must encode");
    assert_eq!(
        commit.digest.to_string(),
        "c815dde2650f4875b04bc4db1155abc91686a1ac27ea9356be22a98eb41979ea"
    );

    let request = br#"{"model":"test-model","messages":[]}"#;
    assert_eq!(
        Digest::hash(b"mage:provider-request:v1", request).to_string(),
        "db142de905c25d7d75911db9c1a0e92d1f9338e448a7c2ade67ec47f2ca63154"
    );

    let observation = WorkspaceObservation::new(
        fixture::id(20),
        fixture::scope().id(),
        PathPlatform::Unix,
        vec![ObservedEntry::new(
            fixture::unix_path(&[b"src".as_slice(), b"lib.rs".as_slice()]),
            ObservedKind::File {
                content: fixture::digest("lib.rs"),
                executable: false,
            },
        )],
    )
    .expect("observation must be canonical");
    assert_eq!(
        observation.digest().to_string(),
        "779190394f6434fcff575b958da3dee8c487ec82fd36f950261e53eac1727075"
    );

    let manifest = ChangeManifest::new(vec![ChangeEntry::new(
        fixture::unix_path(&[b"src".as_slice(), b"lib.rs".as_slice()]),
        ChangeKind::Modified,
        Some(ObservedKind::File {
            content: fixture::digest("old"),
            executable: false,
        }),
        Some(ObservedKind::File {
            content: fixture::digest("new"),
            executable: true,
        }),
    )])
    .expect("manifest must be canonical");
    assert_eq!(
        manifest.digest().to_string(),
        "36a844b95b71a7d8f1e7da97fc0a2a79c2636e5725c44e4def3369b7979d4710"
    );

    let encoded = serde_json::to_vec(&fixture::head()).expect("request head must encode");
    let decoded: RequestHead = serde_json::from_slice(&encoded).expect("request head must decode");
    assert_eq!(decoded, fixture::head());
    let empty: Items<u8, 1> = Items::default();
    assert!(empty.is_empty());
}
