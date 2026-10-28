use super::*;

#[test]
fn a_project_gets_its_check_from_its_language_data() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(check(dir.path()), None);
    std::fs::write(dir.path().join("package.json"), r#"{"name":"x"}"#).unwrap();
    assert_eq!(check(dir.path()), None);
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"test":"node --test"}}"#,
    )
    .unwrap();
    assert_eq!(check(dir.path()), Some("npm test"));
    let rust = tempfile::tempdir().unwrap();
    std::fs::write(rust.path().join("Cargo.toml"), "[package]\n").unwrap();
    assert_eq!(check(rust.path()), Some("cargo test"));
}
