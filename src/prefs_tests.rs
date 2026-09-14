use super::*;
use std::fs::{read_to_string, write};

#[test]
fn compact_output_preserves_utf8_and_full_output_preserves_bytes() {
    let output = format!("early error\n{}\n\n", "界".repeat(121));
    assert_eq!(ToolOutput::Compact.text(&output), "界".repeat(120));
    assert_eq!(ToolOutput::Full.text(&output), output);
    assert_eq!(ToolOutput::Compact.text("\n  \n"), "");
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mage-prefs-{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("create preference directory: {error}"));
    dir.join("config.toml")
}

#[test]
fn roundtrip_preserves_sections_and_replaces_keys() {
    let path = tmp("roundtrip");
    let original = "# keep this comment\ndefault = \"glm\"\nunknown = \"keep\"\ntheme = \"stale\"\n\n[endpoint.local]\nbase_url = \"http://x\"\ntheme = \"inner\"\n";
    std::fs::write(&path, original).unwrap_or_else(|error| panic!("write preferences: {error}"));
    let prefs = Prefs {
        theme: "fantasy".to_string(),
        splash: false,
        animations: false,
        reasoning: Reasoning::Expanded,
        receipts: Receipts::Expanded,
        tools: ToolOutput::Full,
        bus: false,
        layout: "chat".to_string(),
        hotkeys: Hotkeys::default(),
        last_endpoint: Some("glm".to_string()),
    };
    save_in(&path, &prefs).unwrap_or_else(|error| panic!("save preferences: {error}"));
    let saved = std::fs::read_to_string(&path).unwrap_or_default();
    let top: Vec<&str> = saved
        .lines()
        .take_while(|l| !l.trim_start().starts_with('['))
        .collect();
    assert_eq!(top.iter().filter(|l| l.starts_with("theme")).count(), 1);
    assert!(top.contains(&"default = \"glm\""));
    assert!(saved.contains("# keep this comment"));
    assert!(top.contains(&"unknown = \"keep\""));
    assert!(top.contains(&"splash = false"));
    assert!(top.contains(&"animations = false"));
    assert!(top.contains(&"tools = \"full\""));
    assert!(top.iter().any(|line| line.starts_with("hotkeys = ")));
    assert!(saved.contains("[endpoint.local]\nbase_url = \"http://x\"\ntheme = \"inner\""));
    assert_eq!(load_from(&path).ok(), Some(prefs));
    drop(std::fs::remove_file(&path));
}

#[test]
fn hotkeys_load_overrides_and_reject_conflicts() {
    let path = tmp("hotkeys");
    std::fs::write(
        &path,
        "hotkeys = { options = \"ctrl+k\", files = \"ctrl+o\" }\n",
    )
    .unwrap_or_else(|error| panic!("write hotkeys: {error}"));
    let prefs = load_from(&path).unwrap_or_else(|error| panic!("load hotkeys: {error}"));
    assert_eq!(prefs.hotkeys.label(Hotkey::Options), "ctrl+k");
    assert_eq!(prefs.hotkeys.label(Hotkey::Files), "ctrl+o");

    std::fs::write(&path, "hotkeys = { options = \"ctrl+e\" }\n")
        .unwrap_or_else(|error| panic!("write conflict: {error}"));
    let error = load_from(&path)
        .expect_err("duplicate hotkey must fail")
        .to_string();
    assert!(error.contains("assigned more than once"), "{error}");
    drop(std::fs::remove_file(&path));
}

#[test]
fn adding_search_preserves_an_existing_ctrl_f_binding() {
    let mut configured = BTreeMap::from([("options".to_string(), "ctrl+f".to_string())]);
    let keys = Hotkeys::configured(&configured).expect("existing settings load");
    assert_eq!(keys.action('f'), Some(Hotkey::Options));
    assert_ne!(keys.key(Hotkey::Search), 'f');
    assert_eq!(keys.action(keys.key(Hotkey::Search)), Some(Hotkey::Search));
    assert_eq!(keys.key(Hotkey::Search), 'o');
    configured.insert("search".to_string(), "ctrl+f".to_string());
    assert!(
        Hotkeys::configured(&configured)
            .expect_err("explicit conflict")
            .to_string()
            .contains("assigned more than once")
    );
}

#[test]
fn adding_history_preserves_an_existing_ctrl_r_binding() {
    let configured = BTreeMap::from([("copy".to_string(), "ctrl+r".to_string())]);
    let keys = Hotkeys::configured(&configured).expect("existing settings load");
    assert_eq!(keys.action('r'), Some(Hotkey::Copy));
    assert_eq!(keys.key(Hotkey::History), 'b');
    assert_eq!(keys.action('a'), None, "migration preserves select all");
}

#[test]
fn saved_hotkeys_release_redo_and_preserve_explicit_bindings() {
    let dir = tempfile::tempdir().expect("settings directory");
    let path = dir.path().join("config.toml");
    let legacy = "hotkeys = { options = \"ctrl+o\", newline = \"ctrl+j\", editor = \"ctrl+g\", files = \"ctrl+e\", ledger = \"ctrl+l\", diff = \"ctrl+x\", copy = \"ctrl+y\", select = \"ctrl+t\", pane = \"ctrl+p\", search = \"ctrl+f\", history = \"ctrl+r\" }\n";
    write(&path, legacy).expect("old settings");
    let mut prefs = load_from(&path).expect("load old settings");
    assert_eq!(prefs.hotkeys, Hotkeys::default());
    assert_eq!(prefs.hotkeys.key(Hotkey::Copy), 'b');
    assert_eq!(prefs.hotkeys.action('y'), None);
    assert!(!prefs.hotkeys.assign(Hotkey::Copy, 'y'));
    save_in(&path, &prefs).expect("save migrated settings");
    assert_eq!(load_from(&path).expect("reload migrated settings"), prefs);
    assert!(
        !read_to_string(&path)
            .expect("saved settings")
            .contains("ctrl+y")
    );
    write(
        &path,
        "hotkeys = { options = \"ctrl+b\", copy = \"ctrl+y\" }\n",
    )
    .expect("conflicting destination");
    let prefs = load_from(&path).expect("preserved custom shortcut");
    assert_eq!(prefs.hotkeys.key(Hotkey::Options), 'b');
    assert_eq!(prefs.hotkeys.key(Hotkey::Copy), 'o');
    assert_eq!(prefs.hotkeys.action('y'), None);
    write(&path, "hotkeys = { files = \"ctrl+b\" }\n").expect("custom shortcut");
    let prefs = load_from(&path).expect("move inherited copy shortcut");
    assert_eq!(prefs.hotkeys.key(Hotkey::Files), 'b');
    assert_eq!(prefs.hotkeys.key(Hotkey::Copy), 'e');
    write(&path, "hotkeys = { newline = \"ctrl+y\" }\n").expect("legacy newline");
    let prefs = load_from(&path).expect("migrate reserved newline");
    assert_eq!(prefs.hotkeys.key(Hotkey::Newline), 'j');
    assert_eq!(prefs.hotkeys.action('a'), None);
    write(
        &path,
        "hotkeys = { copy = \"ctrl+y\", files = \"ctrl+y\" }\n",
    )
    .expect("invalid duplicate");
    assert!(
        load_from(&path)
            .expect_err("duplicate remains invalid")
            .to_string()
            .contains("assigned more than once")
    );
}

#[test]
fn assigning_a_used_hotkey_swaps_the_actions() {
    let mut hotkeys = Hotkeys::default();

    assert!(hotkeys.assign(Hotkey::Options, 'e'));
    assert_eq!(hotkeys.label(Hotkey::Options), "ctrl+e");
    assert_eq!(hotkeys.label(Hotkey::Files), "ctrl+o");
    assert!(!hotkeys.assign(Hotkey::Options, 'c'));
}

#[test]
fn keys_land_before_the_first_section() {
    let path = tmp("insert");
    std::fs::write(&path, "[endpoint.local]\nbase_url = \"http://x\"\n")
        .unwrap_or_else(|error| panic!("write endpoint preferences: {error}"));
    save_in(&path, &Prefs::default())
        .unwrap_or_else(|error| panic!("save default preferences: {error}"));
    let saved = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(saved.lines().next(), Some("theme = \"space\""));
    assert!(
        saved
            .lines()
            .nth(KEYS.len())
            .is_some_and(|l| l.starts_with('['))
    );
    drop(std::fs::remove_file(&path));
}

#[test]
fn missing_file_uses_defaults_and_bad_values_fail() {
    let path = tmp("missing");
    assert_eq!(load_from(&path).ok(), Some(Prefs::default()));
    std::fs::write(&path, "splash = maybe\ntools = \"huge\"\n")
        .unwrap_or_else(|error| panic!("write invalid preferences: {error}"));
    load_from(&path).unwrap_err();
    drop(std::fs::remove_file(&path));
}

#[test]
fn receipts_cycle_off_collapsed_expanded() {
    assert_eq!(Receipts::default(), Receipts::Off);
    assert_eq!(Receipts::parse("collapsed"), Receipts::Collapsed);
    assert_eq!(Receipts::parse("EXPANDED"), Receipts::Expanded);
    assert_eq!(Receipts::parse("nonsense"), Receipts::Off);
    assert_eq!(Receipts::Off.toggle(), Receipts::Collapsed);
    assert_eq!(Receipts::Expanded.toggle(), Receipts::Off);
}

#[test]
fn reasoning_defaults_collapsed_and_cycles() {
    assert_eq!(Reasoning::default(), Reasoning::Collapsed);
    assert_eq!(Reasoning::parse("off"), Reasoning::Off);
    assert_eq!(Reasoning::parse("shown"), Reasoning::Expanded);
    assert_eq!(Reasoning::parse("nonsense"), Reasoning::Collapsed);
    assert_eq!(Reasoning::Off.toggle(), Reasoning::Collapsed);
    assert_eq!(Reasoning::Expanded.toggle(), Reasoning::Off);
}
