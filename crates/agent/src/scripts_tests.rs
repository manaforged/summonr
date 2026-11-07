use std::path::Path;

use super::{Script, Scripts};

fn write(root: &Path, file: impl AsRef<Path>, text: &str) {
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn seen(root: &Path) -> [Option<String>; 3] {
    let scripts = Scripts::load(root).unwrap();
    [Script::Setup, Script::Run, Script::Archive]
        .map(|script| scripts.get(script).map(str::to_string))
}

fn some(text: &str) -> Option<String> {
    Some(text.to_string())
}

#[test]
fn the_summonr_file_wins_whole_over_every_conductor_file() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        ".conductor/settings.toml",
        "[scripts]\nsetup = \"shared\"\nrun = \"shared run\"\n",
    );
    write(
        root.path(),
        "conductor.json",
        r#"{"scripts": {"archive": "legacy"}}"#,
    );
    write(
        root.path(),
        Path::new(".summonr").join("scripts.toml"),
        "setup = \"own\"\n",
    );
    assert_eq!(seen(root.path()), [some("own"), None, None]);
}

#[test]
fn conductor_local_settings_override_shared_ones_key_by_key() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        ".conductor/settings.toml",
        "[scripts]\nsetup = \"shared\"\narchive = \"shared archive\"\nrun_mode = \"concurrent\"\n\n[scripts.run.web]\ncommand = \"pnpm dev\"\n",
    );
    write(
        root.path(),
        ".conductor/settings.local.toml",
        "[scripts]\nsetup = \"local\"\n",
    );
    write(
        root.path(),
        "conductor.json",
        r#"{"scripts": {"run": "legacy"}}"#,
    );
    assert_eq!(
        seen(root.path()),
        [some("local"), None, some("shared archive")]
    );
}

#[test]
fn the_legacy_conductor_json_is_read_when_no_settings_file_exists() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(seen(root.path()), [None, None, None]);
    write(
        root.path(),
        "conductor.json",
        r#"{"scripts": {"setup": "npm ci", "run": "npm start"}}"#,
    );
    assert_eq!(seen(root.path()), [some("npm ci"), some("npm start"), None]);
}

#[test]
fn a_broken_scripts_file_names_its_path() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), ".conductor/settings.toml", "[scripts\n");
    let error = Scripts::load(root.path()).unwrap_err().to_string();
    assert!(error.contains("settings.toml"), "{error}");
}
