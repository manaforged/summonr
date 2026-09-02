use super::*;

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
        reasoning: true,
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
