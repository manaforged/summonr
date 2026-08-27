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
