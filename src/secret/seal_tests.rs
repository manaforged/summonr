use super::*;

#[test]
fn each_key_source_has_its_own_label() {
    let labels = [
        (Source::Env, "env"),
        (Source::Keychain, "keychain"),
        (Source::File, "file"),
        (Source::Config, "config (plaintext)"),
        (Source::Builtin, "built-in"),
    ];
    for (source, label) in labels {
        assert_eq!(source.label(), label);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_written_wrap_key_is_read_back_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let credentials = dir.path().join("home").join("credentials.toml");
    let key = [9_u8; KEY_LEN];
    write_wrap(&wrap_path(&credentials), &key).unwrap();
    assert_eq!(file_wrap(&credentials).unwrap(), key);
}
