use super::*;

#[test]
fn workspace_root_has_an_empty_relative_key() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let key = relative_key(directory.path(), directory.path()).expect("root must be relative");

    assert_eq!(key, PathKey::root(platform()));
}
