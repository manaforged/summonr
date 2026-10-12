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

#[test]
fn an_old_last_endpoint_key_loads_and_the_next_save_drops_it() {
    let (_dir, path) = written("last_endpoint = \"zai\"\ntheme = \"space\"\n");
    parse(&path).unwrap();
    let prefs = crate::prefs::load_from(&path).unwrap();
    crate::prefs::save_in(&path, &prefs).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("last_endpoint"), "{text}");
    assert!(text.contains("theme"), "{text}");
}

#[test]
fn a_retired_plan_mode_key_is_not_called_unknown_and_the_next_save_drops_it() {
    let (_dir, path) = written("plan_mode = false\ntheme = \"space\"\n");
    let file = parse(&path).unwrap();
    assert!(!file.other.contains_key("plan_mode"), "{:?}", file.other);
    let prefs = crate::prefs::load_from(&path).unwrap();
    crate::prefs::save_in(&path, &prefs).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("plan_mode"), "{text}");
}

#[test]
fn retired_ban_keys_are_not_called_unknown_and_the_next_save_drops_them() {
    let (_dir, path) = written("ban_comments = true\nban_tests = false\ntheme = \"space\"\n");
    let file = parse(&path).unwrap();
    assert!(!file.other.contains_key("ban_comments"), "{:?}", file.other);
    assert!(!file.other.contains_key("ban_tests"), "{:?}", file.other);
    let prefs = crate::prefs::load_from(&path).unwrap();
    crate::prefs::save_in(&path, &prefs).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("ban_"), "{text}");
}

#[test]
fn module_switches_load_by_id_and_unknown_names_do_not_break_the_file() {
    let (_dir, path) =
        written("[modules]\nmail = true\nbogus = false\n\n[sessions]\nretain_days = 2\n");
    let file = parse(&path).unwrap();
    assert_eq!(
        file.modules,
        [(crate::modules::ModuleId::Mail, true)].into()
    );
    assert_eq!(
        file.sessions.retain(),
        std::time::Duration::from_secs(2 * 24 * 60 * 60)
    );
    assert!(
        crate::modules::Switches::from_maps(&file.modules, &std::collections::BTreeMap::new())
            .enabled(crate::modules::ModuleId::Mail)
    );
}

#[test]
fn the_terminal_background_survives_a_save() {
    let (_dir, path) = written("background = \"terminal\"\n");
    let prefs = crate::prefs::load_from(&path).unwrap();
    assert_eq!(prefs.background, crate::prefs::Background::Terminal);
    crate::prefs::save_in(&path, &prefs).unwrap();
    assert_eq!(
        crate::prefs::load_from(&path).unwrap().background,
        crate::prefs::Background::Terminal
    );
}

#[test]
fn a_retired_theme_name_loads_and_an_unknown_one_is_refused() {
    let (_dir, path) = written("theme = \"solarized\"\n");
    drop(crate::prefs::load_from(&path).unwrap());
    let (_dir, path) = written("theme = \"auto\"\n");
    drop(parse(&path).unwrap());
    let (_dir, path) = written("theme = \"pink\"\n");
    drop(parse(&path).unwrap_err());
}

#[test]
fn the_notify_setting_survives_a_save() {
    let (_dir, path) = written("notify = \"all\"\n");
    let prefs = crate::prefs::load_from(&path).unwrap();
    assert_eq!(prefs.notify, crate::prefs::Notify::All);
    crate::prefs::save_in(&path, &prefs).unwrap();
    assert_eq!(
        crate::prefs::load_from(&path).unwrap().notify,
        crate::prefs::Notify::All
    );
}

#[test]
fn an_old_hotkey_table_that_binds_ctrl_n_still_loads() {
    let (_dir, path) = written("[hotkeys]\npane = \"ctrl+n\"\n");
    let prefs = crate::prefs::load_from(&path).unwrap();
    assert_eq!(prefs.hotkeys.action('n'), Some(crate::prefs::Hotkey::Pane));
    assert_eq!(prefs.hotkeys.chord('n'), Some(crate::prefs::Hotkey::Fold));
}

#[test]
fn a_save_writes_only_the_settings_that_changed() {
    let (_dir, path) = written("default = \"zai\"\n");
    let mut prefs = crate::prefs::load_from(&path).unwrap();
    crate::prefs::save_in(&path, &prefs).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text, "default = \"zai\"\n");
    prefs.layout = "chat | mcp 40%".to_string();
    crate::prefs::save_in(&path, &prefs).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("layout = \"chat | mcp 40%\""), "{text}");
    for key in ["theme", "fps", "splash", "hotkeys"] {
        assert!(!text.contains(key), "{key}: {text}");
    }
}
