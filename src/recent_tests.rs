use super::*;

#[test]
fn a_model_remembers_its_effort() {
    let home = tempfile::tempdir().unwrap();
    let selection = ModelId::new("zai", "glm");
    record(home.path(), &selection, Some(Effort::Medium)).unwrap();
    record(home.path(), &ModelId::new("go", "m"), None).unwrap();
    assert_eq!(
        remembered_effort(Some(home.path()), "zai", "glm"),
        Some(Effort::Medium)
    );
    assert_eq!(remembered_effort(Some(home.path()), "go", "m"), None);
    assert_eq!(load(home.path())[1], selection);
}

#[test]
fn a_row_written_before_effort_still_loads() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(FILE), r#"["zai\tglm"]"#).unwrap();
    assert_eq!(load(home.path()), [ModelId::new("zai", "glm")]);
    assert_eq!(remembered_effort(Some(home.path()), "zai", "glm"), None);
}
