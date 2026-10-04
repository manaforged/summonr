use std::os::unix::fs::PermissionsExt;

use super::*;

#[test]
fn an_unwritable_home_opens_the_session_in_temp() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let session = fresh(
        Ok(home.path().to_path_buf()),
        workspace.path(),
        "model",
        "system",
    );
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let session = session.unwrap();
    assert!(
        session.path().starts_with(std::env::temp_dir()),
        "{}",
        session.path().display()
    );
    assert!(!session.path().starts_with(home.path()));
    drop(std::fs::remove_dir_all(
        std::env::temp_dir().join(format!("summonr-{}", std::process::id())),
    ));
}
