use super::*;

#[test]
fn save_key_preserves_endpoint_settings() {
    let home = tempfile::tempdir().expect("temp home");
    let path = home.path().join("config.toml");
    std::fs::write(&path, "[endpoint.glm]\nmodel = \"glm-5.3-flash\"\n").expect("seed config");

    save_key(&path, "glm", "new-secret").expect("save key");

    let file = load_from(&path).expect("load config");
    let endpoint = file.endpoint.get("glm").expect("glm endpoint");
    assert_eq!(endpoint.model.as_deref(), Some("glm-5.3-flash"));
    assert_eq!(endpoint.api_key.as_deref(), Some("new-secret"));
}

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
