use super::*;

#[test]
fn a_new_session_index_starts_from_the_repository_index() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(workspace)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(workspace.join("a.txt"), "a\n").unwrap();
    git(&["add", "a.txt"]);
    let checkpoints = Checkpoints::open(workspace, "s1").unwrap();
    assert_eq!(
        std::fs::read(&checkpoints.index).unwrap(),
        std::fs::read(workspace.join(".git/index")).unwrap()
    );
}
