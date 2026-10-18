use super::*;

#[test]
fn write_keeps_a_symlinked_file_shared() {
    let dir = tempfile::tempdir().unwrap();
    let shared = dir.path().join("shared.toml");
    let link = dir.path().join("credentials.toml");
    std::fs::write(&shared, "old").unwrap();
    std::os::unix::fs::symlink(&shared, &link).unwrap();

    write(&link, "rotated", Durability::Synced).unwrap();

    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_to_string(&shared).unwrap(), "rotated");
}
