use super::*;

#[cfg(target_os = "macos")]
#[test]
fn a_written_wrap_key_is_read_back_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let credentials = dir.path().join("home").join("credentials.toml");
    let key = [9_u8; KEY_LEN];
    write_wrap(&wrap_path(&credentials), &key).unwrap();
    assert_eq!(file_wrap(&credentials).unwrap(), key);
}
