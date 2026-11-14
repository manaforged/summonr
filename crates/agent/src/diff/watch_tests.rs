use super::Watch;

const FLUSH: std::time::Duration = std::time::Duration::from_millis(500);

#[test]
fn a_file_written_outside_git_is_reported_and_nothing_else_is() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("untouched.txt"), "a").unwrap();
    std::thread::sleep(FLUSH);
    let watch = Watch::start(dir.path()).expect("a plain folder is watched");
    std::fs::create_dir(dir.path().join("out")).unwrap();
    std::fs::write(dir.path().join("out/made.txt"), "b").unwrap();
    let paths = watch.finish();
    assert!(paths.contains(&"out/made.txt".to_string()), "{paths:?}");
    assert!(!paths.contains(&"untouched.txt".to_string()), "{paths:?}");
}

#[test]
fn a_git_workspace_reports_through_status_instead() {
    let dir = tempfile::tempdir().unwrap();
    crate::diff::git_env(dir.path(), &[], &["init", "-q"]).unwrap();
    assert!(Watch::start(dir.path()).is_none());
}
