use std::path::Path;

#[cfg(unix)]
#[test]
fn a_path_under_a_symlinked_home_reads_from_the_tilde() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real-home");
    std::fs::create_dir_all(real.join("proj")).unwrap();
    let link = root.path().join("link-home");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let workspace = std::fs::canonicalize(real.join("proj")).unwrap();
    assert_eq!(super::under(&workspace, Some(&link)), "~/proj");
    assert_eq!(super::under(&workspace, Some(&real)), "~/proj");
}

#[test]
fn a_path_outside_home_stays_whole() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = root.path().join("elsewhere");
    assert_eq!(
        super::under(&elsewhere, Some(Path::new("/nonexistent-home"))),
        elsewhere.display().to_string()
    );
}

#[test]
fn home_itself_reads_as_a_bare_tilde() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(super::under(root.path(), Some(root.path())), "~");
    assert_eq!(
        super::under(&root.path().join("code"), Some(root.path())),
        "~/code"
    );
}
