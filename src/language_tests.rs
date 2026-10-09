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
