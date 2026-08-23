use super::{ConvertError, plan};

fn is_malformed(result: Result<Option<String>, ConvertError>) -> bool {
    matches!(result, Err(ConvertError::Malformed))
}

fn workspace() -> tempfile::TempDir {
    tempfile::tempdir().expect("temporary workspace must exist")
}

#[test]
fn absent_plan_is_no_plan() {
    let workspace = workspace();
    assert!(matches!(plan(workspace.path()), Ok(None)));
}

#[test]
fn well_formed_markdown_converts() {
    let workspace = workspace();
    crate::plan::write(workspace.path(), "# Plan\n\n- ship import\n")
        .expect("plan file must write");
    let converted = plan(workspace.path()).expect("well-formed plan must convert");
    assert_eq!(converted.as_deref(), Some("# Plan\n\n- ship import\n"));
}

#[test]
fn empty_present_file_blocks() {
    let workspace = workspace();
    crate::plan::write(workspace.path(), "   \n").expect("empty plan file must write");
    assert!(is_malformed(plan(workspace.path())));
}

#[test]
fn directory_at_plan_path_blocks() {
    let workspace = workspace();
    std::fs::create_dir_all(workspace.path().join(".mage/plan.md"))
        .expect("directory must be created");
    assert!(is_malformed(plan(workspace.path())));
}

#[test]
fn invalid_utf8_blocks() {
    let workspace = workspace();
    let path = crate::plan::path(workspace.path());
    std::fs::create_dir_all(path.parent().expect("plan path has a parent"))
        .expect("plan parent must exist");
    std::fs::write(&path, [0xff, 0xfe]).expect("invalid bytes must write");
    assert!(is_malformed(plan(workspace.path())));
}
