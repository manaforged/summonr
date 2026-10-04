use std::os::unix::fs::PermissionsExt;

use super::*;

#[test]
fn an_unwritable_home_opens_the_session_in_a_private_temp_home() {
    if crate::policy::running_as_root() {
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let first = fresh(
        Ok(home.path().to_path_buf()),
        workspace.path(),
        "model",
        "system",
    );
    let second = fresh(
        Ok(home.path().to_path_buf()),
        workspace.path(),
        "model",
        "system",
    );
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (first, second) = (first.unwrap(), second.unwrap());
    let dir = ephemeral().unwrap();
    assert!(first.path().starts_with(dir), "{}", first.path().display());
    assert!(first.path().exists() && second.path().exists());
    let mode = std::fs::metadata(dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    drop(std::fs::remove_dir_all(dir));
}
