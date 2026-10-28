use super::*;

fn vault(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "vault")
}

fn names(walk: Walk, root: &Path) -> Vec<String> {
    let mut names: Vec<String> = walk
        .entries(root)
        .filter(listed)
        .map(|entry| {
            entry
                .path()
                .strip_prefix(root)
                .unwrap()
                .display()
                .to_string()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn a_guarded_walk_never_enters_a_directory_the_guard_protects() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("vault")).unwrap();
    std::fs::write(dir.path().join("vault/key"), "secret").unwrap();
    std::fs::write(dir.path().join("notes.txt"), "plain").unwrap();
    let guarded = Walk {
        guard: Some(vault),
        ..Walk::RAW
    };
    assert_eq!(names(guarded, dir.path()), ["notes.txt"]);
    assert_eq!(names(Walk::RAW, dir.path()), ["notes.txt", "vault/key"]);
}
