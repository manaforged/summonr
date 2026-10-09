use super::*;

#[test]
fn rust_analyzer_starts_with_its_cheap_settings_and_files_get_their_ids() {
    let rust = options(named("rust").unwrap());
    assert_eq!(rust["procMacro"]["enable"], json!(false), "{rust}");
    assert_eq!(rust["checkOnSave"], json!(false), "{rust}");
    assert_eq!(options(named("go").unwrap()), json!({}));
    assert_eq!(language_id(Path::new("a/b.tsx")), "typescriptreact");
    assert_eq!(language_id(Path::new("Cargo.toml")), "toml");
    assert_eq!(language_id(Path::new("README")), "plaintext");
}

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
