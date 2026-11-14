use std::process::{Command, Stdio};

use serde_json::json;

#[test]
fn doctor_report_shows_the_last_crash_with_home_paths_hidden() {
    let home = tempfile::tempdir().unwrap();
    let logs = home.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let root = home.path().display().to_string();
    let lines = [
        json!({"time_unix_ms": 1, "run": "1-1", "event": "start", "cwd": format!("{root}/work/app"), "version": "0.1.0"}),
        json!({"time_unix_ms": 2, "run": "1-1", "event": "panic", "message": format!("panicked at {root}/src/turn.rs:9:5: index out of range"), "backtrace": "0: summonr::turn"}),
    ];
    let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
    std::fs::write(logs.join("1-1.jsonl"), body).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(["doctor", "--report"])
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("SUMMONR_HOME", home.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("index out of range"), "{text}");
    assert!(text.contains("panic"), "{text}");
    assert!(text.contains("~/work/app"), "{text}");
    assert!(
        !text.contains(&root),
        "the report leaks the home path:\n{text}"
    );
}

#[test]
fn doctor_names_the_claude_settings_files_only_when_imported() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(work.path().join(".claude")).unwrap();
    std::fs::write(work.path().join(".claude/settings.json"), "{}").unwrap();
    for (config, shown) in [
        ("", "claude settings\tnone"),
        ("import_claude_settings = true\n", ".claude/settings.json"),
    ] {
        std::fs::write(home.path().join("config.toml"), config).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
            .arg("doctor")
            .current_dir(work.path())
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("SUMMONR_HOME", home.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains(shown), "{shown}: {text}");
    }
}
