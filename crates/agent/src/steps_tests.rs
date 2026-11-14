const PLAN: &str = "## Steps\n\n1. Add the flag\n";

#[test]
fn a_plan_is_named_inside_the_workspace_and_by_full_path_outside_it() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("docs")).unwrap();
    let inside = workspace.path().join("docs/PLAN.md");
    let away = outside.path().join("PLAN.md");
    std::fs::write(&inside, PLAN).unwrap();
    std::fs::write(&away, PLAN).unwrap();
    let (name, _, steps) = super::load(workspace.path(), &inside).unwrap();
    assert_eq!((name.as_str(), steps.len()), ("docs/PLAN.md", 1));
    let (name, _, _) = super::load(workspace.path(), &away).unwrap();
    assert_eq!(name, away.to_string_lossy());
}
