use super::*;

#[test]
fn endpoint_rows_carry_effort_and_price() {
    let home = tempfile::tempdir().expect("temp home");
    let path = home.path().join("config.toml");
    std::fs::write(
        &path,
        "[endpoint.ds]\nmodel = \"deepseek-v4-flash\"\neffort = \"low\"\nprice = { input = 0.14, output = 0.28, cached = 0.014 }\n",
    )
    .expect("seed config");
    let file = load_from(&path).expect("load config");
    let row = file.endpoint.get("ds").expect("ds endpoint");
    assert_eq!(row.effort, Some(crate::provider::Effort::Low));
    let price = row.price.expect("price");
    assert!((price.input - 0.14).abs() < 1e-9 && (price.cached - 0.014).abs() < 1e-9);

    std::fs::write(&path, "[endpoint.ds]\neffort = \"turbo\"\n").expect("seed config");
    let error = load_from(&path).expect_err("bad effort").to_string();
    assert!(
        error.contains("turbo") || error.contains("effort"),
        "{error}"
    );
}

#[test]
fn remove_key_strips_only_the_key() {
    let home = tempfile::tempdir().expect("temp home");
    let path = home.path().join("config.toml");
    std::fs::write(
        &path,
        "[endpoint.glm]\nmodel = \"glm-5.3\"\napi_key = \"secret\"\n",
    )
    .expect("seed config");
    assert!(remove_key(&path, "glm").expect("remove"));
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(!text.contains("secret"), "{text}");
    assert!(text.contains("model = \"glm-5.3\""), "{text}");
    assert!(!remove_key(&path, "glm").expect("nothing left"));
    assert!(!remove_key(&home.path().join("missing.toml"), "glm").expect("no file"));
}

#[test]
fn secrets_accepts_file_and_keychain_only() {
    let home = tempfile::tempdir().expect("temp home");
    let path = home.path().join("config.toml");
    std::fs::write(&path, "secrets = \"keychain\"\n").expect("seed");
    assert_eq!(
        load_from(&path).expect("load").secrets.as_deref(),
        Some("keychain")
    );
    std::fs::write(&path, "secrets = \"disk\"\n").expect("seed");
    let error = load_from(&path).expect_err("bad secrets").to_string();
    assert!(
        error.contains("file") && error.contains("keychain"),
        "{error}"
    );
}

#[test]
fn lsp_accepts_one_curated_module_or_global_command() {
    let home = tempfile::tempdir().expect("temp home");
    let path = home.path().join("config.toml");
    std::fs::write(&path, "[lsp]\nmodule = \"rust\"\n").expect("seed module");
    assert_eq!(
        load_from(&path).expect("load module").lsp.module.as_deref(),
        Some("rust")
    );

    std::fs::write(
        &path,
        "[lsp]\nmodule = \"rust\"\ncommand = [\"rust-analyzer\"]\n",
    )
    .expect("seed conflict");
    let conflict = load_from(&path)
        .expect_err("conflict must fail")
        .to_string();
    assert!(conflict.contains("not both"), "{conflict}");

    std::fs::write(&path, "[lsp]\nmodule = \"java\"\n").expect("seed unknown");
    let unknown = load_from(&path).expect_err("unknown must fail").to_string();
    assert!(
        unknown.contains("unknown language module `java`"),
        "{unknown}"
    );
}
