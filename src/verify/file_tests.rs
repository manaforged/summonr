use super::*;

fn workspace(check_file: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".summonr")).unwrap();
    std::fs::write(dir.path().join(".summonr/check.toml"), check_file).unwrap();
    dir
}

#[test]
fn a_metric_alone_turns_the_check_gate_on_with_no_commands() {
    let dir = workspace("[metric]\ncommand = \"bench\"\nbetter = \"lower\"\n");
    assert_eq!(check_commands(dir.path()).unwrap(), Some(Vec::new()));
    assert_eq!(metric(dir.path()).unwrap().unwrap().command, "bench");
}

#[test]
fn a_bad_metric_pattern_or_an_unknown_metric_key_is_a_config_error() {
    let bad = workspace("[metric]\ncommand = \"b\"\nbetter = \"lower\"\npattern = \"(\"\n");
    let error = check_commands(bad.path()).unwrap_err().to_string();
    assert!(error.contains("metric pattern"), "{error}");
    let unknown = workspace("[metric]\ncommand = \"b\"\nbetter = \"lower\"\ntarget_ms = 3\n");
    let error = check_commands(unknown.path()).unwrap_err().to_string();
    assert!(error.contains("target_ms"), "{error}");
}
