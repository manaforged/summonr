use super::snapshot;

#[test]
fn a_file_another_handle_locked_does_not_stop_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("open.txt"), "a").unwrap();
    std::fs::write(dir.path().join("held.lock"), "a").unwrap();
    let held = std::fs::File::options()
        .read(true)
        .write(true)
        .open(dir.path().join("held.lock"))
        .unwrap();
    held.lock().unwrap();
    let taken = snapshot(dir.path(), None, None).unwrap();
    assert!(taken.files.contains_key("open.txt"));
    drop(held);
}
