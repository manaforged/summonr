use super::edit::save_default_seat;
use super::parse;

fn written(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, text).unwrap();
    (dir, path)
}

#[test]
fn a_malformed_config_names_its_file_and_line() {
    let (_dir, path) = written("default = \"go\"\nmodel = \"unterminated\n");
    let error = parse(&path).expect_err("a broken file must not load as defaults");
    let text = error.to_string();
    assert!(text.contains("config.toml"), "got {text}");
    assert!(text.contains("line 2"), "got {text}");
}

#[test]
fn setting_the_default_seat_keeps_notes_and_other_rows() {
    let (_dir, path) = written(
        "# my providers\ndefault = \"go\"\n\n[endpoint.local]\nbase_url = \"http://127.0.0.1:8080/v1\"\n",
    );
    save_default_seat(&path, Some(("anthropic", "claude-sonnet-5"))).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# my providers"), "got {text}");
    assert!(text.contains("default = \"anthropic\""), "got {text}");
    assert!(
        text.contains("default_model = \"claude-sonnet-5\""),
        "got {text}"
    );
    assert!(text.contains("[endpoint.local]"), "got {text}");
}

#[test]
fn clearing_the_default_seat_removes_both_keys() {
    let (_dir, path) = written("default = \"go\"\ndefault_model = \"glm\"\n");
    save_default_seat(&path, None).unwrap();
    let file = parse(&path).unwrap();
    assert!(file.default.is_none());
    assert!(file.default_model.is_none());
}

#[test]
fn a_config_that_still_says_bus_loads_after_a_save() {
    let (_dir, path) = written("bus = false\n");
    let prefs = crate::prefs::load_from(&path).unwrap();
    crate::prefs::save_in(&path, &prefs).unwrap();
    let file = parse(&path).unwrap();
    assert_eq!(file.mail, Some(false));
}

#[test]
fn a_config_with_both_mail_and_bus_keeps_mail() {
    let (_dir, path) = written("mail = true\nbus = false\n");
    let file = parse(&path).unwrap();
    assert_eq!(file.mail, Some(true));
}
