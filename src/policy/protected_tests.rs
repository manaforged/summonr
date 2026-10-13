#[cfg(unix)]
#[test]
fn a_protected_write_is_refused_through_a_verbatim_or_symlinked_home() {
    let real = tempfile::tempdir().expect("home");
    let links = tempfile::tempdir().expect("links");
    let home = links.path().join("home");
    std::os::unix::fs::symlink(real.path(), &home).expect("symlink");
    std::fs::create_dir(real.path().join(".ssh")).expect("ssh");
    let keys = real
        .path()
        .canonicalize()
        .expect("real")
        .join(".ssh/authorized_keys");
    assert!(super::secret_under(&keys, &home, &[".ssh"]));
    assert!(!super::secret_under(
        &real.path().join("notes.txt"),
        &home,
        &[".ssh"]
    ));
}
