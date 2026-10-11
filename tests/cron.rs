use std::path::Path;
use std::process::{Command, Stdio};

fn cron(home: &Path, env: &[(&str, &str)], args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .arg("cron")
        .args(args)
        .current_dir(home)
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("SUMMONR_HOME", home.join(".summonr"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(std::env::var_os("SYSTEMROOT").map(|root| ("SYSTEMROOT", root)))
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), text)
}

#[test]
fn a_job_is_added_listed_and_removed() {
    let home = tempfile::tempdir().unwrap();
    let (code, text) = cron(
        home.path(),
        &[],
        &[
            "add",
            "nightly",
            "--cron",
            "0 3 * * *",
            "--goal",
            "make the tests pass",
            "--tz",
            "America/Chicago",
        ],
    );
    assert_eq!(code, 0, "{text}");
    let (_, list) = cron(home.path(), &[], &["list", "--json"]);
    let job: serde_json::Value = serde_json::from_str(list.trim()).unwrap();
    assert_eq!(job["name"], "nightly");
    assert_eq!(job["schedule"], "0 3 * * *");
    assert_eq!(job["paused"], false);
    let (code, text) = cron(home.path(), &[], &["remove", "nightly"]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(cron(home.path(), &[], &["list"]).1, "");
}

#[test]
fn a_cron_run_cannot_change_jobs() {
    let home = tempfile::tempdir().unwrap();
    let (code, text) = cron(
        home.path(),
        &[("SUMMONR_CRON", "nightly")],
        &["add", "more", "--every", "1m", "-p", "hi"],
    );
    assert_ne!(code, 0);
    assert!(
        text.contains("a cron run cannot change cron jobs"),
        "{text}"
    );
    assert_eq!(cron(home.path(), &[], &["list"]).1, "");
}

#[test]
fn a_bad_schedule_is_refused_at_add() {
    let home = tempfile::tempdir().unwrap();
    let (code, text) = cron(
        home.path(),
        &[],
        &["add", "x", "--cron", "61 * * * *", "-p", "hi"],
    );
    assert_ne!(code, 0);
    assert!(text.contains("bad schedule"), "{text}");
}

#[test]
fn a_run_still_going_skips_the_next_one() {
    let home = tempfile::tempdir().unwrap();
    let (code, text) = cron(
        home.path(),
        &[],
        &["add", "slow", "--every", "1h", "-p", "hi"],
    );
    assert_eq!(code, 0, "{text}");
    let runs = home.path().join(".summonr/cron/runs");
    std::fs::create_dir_all(&runs).unwrap();
    let held = std::fs::File::create(runs.join("slow.lock")).unwrap();
    held.lock().unwrap();
    let (code, text) = cron(home.path(), &[], &["exec", "slow", "--slot", "1"]);
    assert_eq!(code, 1, "{text}");
    let (_, log) = cron(home.path(), &[], &["log", "slow"]);
    assert!(log.contains("overlap"), "{log}");
    drop(held);
}
