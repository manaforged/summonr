#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn panel(home: &Path, global: &[&str], args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(global)
        .arg("panel")
        .args(args)
        .current_dir(home)
        .env_clear()
        .env("HOME", home)
        .env("SUMMONR_HOME", home.join("state"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .stdin(Stdio::null())
        .output()
        .expect("summonr runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), text)
}

fn write_panel(home: &Path, name: &str, script: &str) {
    let dir = home.join(".summonr").join("panels");
    std::fs::create_dir_all(&dir).expect("panel directory");
    let file = format!("title = \"Build times\"\ncommand = [\"sh\", \"-c\", {script:?}]\n");
    std::fs::write(dir.join(format!("{name}.toml")), file).expect("panel file");
}

const CHART: &str = r#"printf '{"view":"chart","kind":"line","labels":["Mon","Tue"],"series":[{"name":"p50","values":[41.0,38.5]}]}'"#;

#[test]
fn check_accepts_a_chart_and_rejects_what_the_app_cannot_draw() {
    let home = tempfile::tempdir().expect("home");
    write_panel(home.path(), "good", CHART);
    let (code, text) = panel(home.path(), &[], &["check", "good"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("good draws a chart of 1 series"), "{text}");

    let cases = [
        ("garbled", "echo not json", "invalid output"),
        ("failing", "echo disk is gone >&2; exit 3", "disk is gone"),
        (
            "unknown",
            r#"printf '{"view":"gauge","value":1}'"#,
            "unknown variant",
        ),
        (
            "short",
            r#"printf '{"view":"chart","labels":["Mon","Tue"],"series":[{"name":"p50","values":[1]}]}'"#,
            "has 1 values for 2 labels",
        ),
    ];
    for (name, script, reason) in cases {
        write_panel(home.path(), name, script);
        let (code, text) = panel(home.path(), &[], &["check", name]);
        assert_ne!(code, 0, "{name}: {text}");
        assert!(text.contains(reason), "{name}: {text}");
    }
}

#[test]
fn check_stops_a_command_that_hangs() {
    let home = tempfile::tempdir().expect("home");
    write_panel(home.path(), "stuck", "sleep 600");
    let state = home.path().join("state");
    std::fs::create_dir_all(&state).expect("state");
    std::fs::write(
        state.join("config.toml"),
        "[limits]\npanel_timeout_secs = 1\n",
    )
    .expect("config");
    let started = Instant::now();
    let (code, text) = panel(home.path(), &[], &["check", "stuck"]);
    assert_ne!(code, 0, "{text}");
    assert!(text.contains("ran past 1s"), "{text}");
    assert!(started.elapsed() < Duration::from_secs(30), "{text}");
}

#[test]
fn an_untrusted_workspace_does_not_run_its_panels() {
    let home = tempfile::tempdir().expect("home");
    let proof = home.path().join("ran");
    let script = format!("touch {}; {CHART}", proof.display());
    write_panel(home.path(), "project", &script);

    let (code, text) = panel(home.path(), &["--no-trust"], &["check", "project"]);
    assert_ne!(code, 0, "{text}");
    assert!(text.contains("no panel named `project`"), "{text}");
    assert!(!proof.exists());

    let (code, text) = panel(home.path(), &[], &["check", "project"]);
    assert_eq!(code, 0, "{text}");
    assert!(proof.exists());
}

#[test]
fn a_broken_panel_file_is_listed_and_check_names_the_problem() {
    let home = tempfile::tempdir().expect("home");
    let dir = home.path().join(".summonr").join("panels");
    std::fs::create_dir_all(&dir).expect("panel directory");
    std::fs::write(dir.join("typo.toml"), "titel = \"x\"\ncommand = [\"y\"]\n").expect("file");
    let (code, text) = panel(home.path(), &[], &["list"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("typo"), "{text}");
    let (code, text) = panel(home.path(), &[], &["check", "typo"]);
    assert_ne!(code, 0, "{text}");
    assert!(text.contains("titel"), "{text}");
}

#[test]
fn help_teaches_the_file_and_every_view() {
    let home = tempfile::tempdir().expect("home");
    let (code, text) = panel(home.path(), &[], &["--help"]);
    assert_eq!(code, 0, "{text}");
    for wanted in [
        "every_secs",
        "\"chart\"",
        "\"stat\"",
        "\"table\"",
        "\"text\"",
        "panel check",
    ] {
        assert!(text.contains(wanted), "{wanted}: {text}");
    }
}

#[test]
fn a_plugin_panel_runs_a_program_from_the_plugin() {
    let home = tempfile::tempdir().expect("home");
    let source = home.path().join("source");
    let panels = source.join("panels");
    let bin = source.join("bin");
    std::fs::create_dir_all(&panels).expect("panels");
    std::fs::create_dir_all(&bin).expect("bin");
    let script = bin.join("show");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '{\"view\":\"text\",\"text\":\"hi\"}'\n",
    )
    .expect("script");
    let mode = std::os::unix::fs::PermissionsExt::from_mode(0o755);
    std::fs::set_permissions(&script, mode).expect("mode");
    std::fs::write(
        panels.join("from-plugin.toml"),
        "command = [\"bin/show\"]\n",
    )
    .expect("file");
    let state = home.path().join("state");
    let installed = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(["plugin", "install"])
        .arg(&source)
        .current_dir(home.path())
        .env_clear()
        .env("HOME", home.path())
        .env("SUMMONR_HOME", &state)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .stdin(Stdio::null())
        .output()
        .expect("install");
    assert!(installed.status.success(), "{installed:?}");
    let (code, text) = panel(home.path(), &[], &["check", "from-plugin"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("draws text"), "{text}");
}
