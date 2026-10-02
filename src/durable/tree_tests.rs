use std::time::{Duration, SystemTime};

use super::{changed, snapshot, walk};

fn backdate(path: &std::path::Path, by: Duration) {
    std::fs::File::open(path)
        .unwrap()
        .set_modified(SystemTime::now() - by)
        .unwrap();
}

#[test]
fn a_same_size_rewrite_is_seen_even_when_it_restores_the_old_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let stale = root.join("stale.txt");
    let fresh = root.join("fresh.txt");
    std::fs::write(&stale, "aaaa").unwrap();
    std::fs::write(&fresh, "aaaa").unwrap();
    backdate(&stale, Duration::from_secs(20));
    backdate(&fresh, Duration::from_secs(20));
    let base = snapshot(root, None, None).unwrap();

    std::fs::write(&stale, "bbbb").unwrap();
    backdate(&stale, Duration::from_secs(20));
    std::fs::write(&fresh, "bbbb").unwrap();
    let capture = snapshot(root, None, Some(&base)).unwrap();

    assert_eq!(
        changed(&base, &capture),
        vec!["fresh.txt".to_string(), "stale.txt".to_string()],
        "a content write moves the change time, so a restored mtime still gets a rehash"
    );
    let again = snapshot(root, None, Some(&capture)).unwrap();
    assert_eq!(
        changed(&capture, &again),
        Vec::<String>::new(),
        "untouched files reuse their marks"
    );
    assert_eq!(
        changed(&base, &snapshot(root, None, None).unwrap()),
        vec!["fresh.txt".to_string(), "stale.txt".to_string()],
        "without a previous snapshot every file is hashed"
    );
}

#[test]
fn the_walk_stays_on_the_workspace_filesystem() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("mounted")).unwrap();
    std::fs::write(root.join("mounted/inside.txt"), "x").unwrap();
    std::fs::write(root.join("top.txt"), "x").unwrap();
    let here = super::device_of(&std::fs::metadata(root).unwrap());
    let mut same = Vec::new();
    walk(root, root, here, &mut same).unwrap();
    same.sort();
    assert_eq!(
        same,
        ["mounted/inside.txt", "top.txt"].map(std::path::PathBuf::from)
    );
    let mut other = Vec::new();
    walk(
        root,
        root,
        here.map(|device| device.wrapping_add(1)),
        &mut other,
    )
    .unwrap();
    assert_eq!(other, ["top.txt"].map(std::path::PathBuf::from));
}
