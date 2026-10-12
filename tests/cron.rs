use std::path::Path;
use std::process::{Command, Stdio};

fn quiet<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut args = args.to_vec();
    if args.first() == Some(&"add") && !args.contains(&"--notify") {
        args.extend(["--notify", "never"]);
    }
    args
}

fn cron(home: &Path, env: &[(&str, &str)], args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .arg("cron")
        .args(quiet(args))
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
    assert!(cron(home.path(), &[], &["list", "--json"]).1.is_empty());
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
    assert!(cron(home.path(), &[], &["list", "--json"]).1.is_empty());
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

#[test]
fn cron_help_and_empty_states_guide_a_new_user() {
    let home = tempfile::tempdir().unwrap();
    let (code, help) = cron(home.path(), &[], &["--help"]);
    assert_eq!(code, 0, "{help}");
    assert!(help.starts_with("usage:"), "{help}");
    let (code, list) = cron(home.path(), &[], &["list"]);
    assert_eq!(code, 0, "{list}");
    assert!(list.contains("no cron jobs; add one with"), "{list}");
    let (code, error) = cron(home.path(), &[], &["add", "x", "-p", "hi"]);
    assert_ne!(code, 0);
    assert!(error.contains("see `summonr cron --help`"), "{error}");
    assert!(!error.contains("usage:"), "{error}");
}

#[test]
fn a_job_reads_as_plain_lines() {
    let home = tempfile::tempdir().unwrap();
    let (code, added) = cron(
        home.path(),
        &[],
        &[
            "add",
            "Nightly",
            "--at",
            "2026-12-01 09:00",
            "--tz",
            "America/Chicago",
            "--goal",
            "ship it",
        ],
    );
    assert_eq!(code, 0, "{added}");
    assert!(
        added.starts_with("added nightly: at 2026-12-01 09:00 CST, next"),
        "{added}"
    );
    let (_, show) = cron(home.path(), &[], &["show", "nightly"]);
    assert!(show.contains("goal: ship it"), "{show}");
    assert!(show.contains("budget: 30m00s (default)"), "{show}");
    assert!(!show.contains("created = "), "{show}");
    let (_, log) = cron(home.path(), &[], &["log", "nightly"]);
    assert_eq!(log.trim(), "no runs yet");
    let (_, token) = cron(home.path(), &[], &["token", "nightly"]);
    assert!(token.contains("Authorization: Bearer"), "{token}");
    assert!(token.contains("/run/nightly"), "{token}");
    let (_, bad) = cron(
        home.path(),
        &[],
        &["add", "y", "--every", "1h", "-p", "hi", "--notify", "loud"],
    );
    assert!(!bad.contains("unknown variant"), "{bad}");
}

#[test]
fn a_cron_run_cannot_read_tokens_or_unpause_jobs_even_through_run_command() {
    let home = tempfile::tempdir().unwrap();
    cron(
        home.path(),
        &[],
        &["add", "job", "--every", "1h", "-p", "hi"],
    );
    for words in [["token", "job"], ["resume", "job"], ["run", "job"]] {
        let (code, text) = cron(home.path(), &[("SUMMONR_CRON", "job")], &words);
        assert_ne!(code, 0, "{words:?}: {text}");
        assert!(
            text.contains("a cron run cannot change cron jobs"),
            "{text}"
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args([
            "tool",
            "run_command",
            r#"{"command":"echo cron=$SUMMONR_CRON"}"#,
        ])
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("SUMMONR_HOME", home.path().join(".summonr"))
        .env("SUMMONR_CRON", "job")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("cron=job"), "{text}");
}

#[test]
fn a_goal_that_reads_as_a_flag_is_refused() {
    let home = tempfile::tempdir().unwrap();
    let (code, text) = cron(
        home.path(),
        &[],
        &["add", "x", "--every", "1h", "--goal", "-rf everything"],
    );
    assert_ne!(code, 0);
    assert!(text.contains("cannot start with `-`"), "{text}");
}

#[test]
fn a_one_shot_in_the_past_is_refused_with_the_formats_that_work() {
    let home = tempfile::tempdir().unwrap();
    let (code, text) = cron(
        home.path(),
        &[],
        &["add", "old", "--at", "2020-01-01 00:00", "-p", "hi"],
    );
    assert_ne!(code, 0);
    assert!(text.contains("past"), "{text}");
    let (code, text) = cron(
        home.path(),
        &[],
        &["add", "bad", "--at", "yesterday", "-p", "hi"],
    );
    assert_ne!(code, 0);
    assert!(
        text.contains("use HH:MM, YYYY-MM-DD HH:MM, or RFC 3339"),
        "{text}"
    );
    assert!(!text.contains("strptime"), "{text}");
    let (code, text) = cron(
        home.path(),
        &[],
        &["add", "soon", "--at", "23:59", "-p", "hi"],
    );
    assert_eq!(code, 0, "{text}");
}

#[test]
fn manual_jobs_never_fire_on_their_own_and_runs_read_as_answers() {
    let home = tempfile::tempdir().unwrap();
    let (code, text) = cron(
        home.path(),
        &[],
        &["add", "alert", "--manual", "-p", "triage"],
    );
    assert_eq!(code, 0, "{text}");
    cron(home.path(), &[], &["tick"]);
    let (_, log) = cron(home.path(), &[], &["log", "alert"]);
    assert!(log.contains("no runs yet"), "{log}");
    cron(home.path(), &[], &["run", "alert"]);
    let (_, log) = cron(home.path(), &[], &["log", "alert"]);
    assert!(!log.contains("{\"type\""), "{log}");
    assert!(log.contains("log: "), "{log}");
    let (_, list) = cron(home.path(), &[], &["list", "--json"]);
    assert!(list.contains("\"paused\":false"), "{list}");
    assert!(list.contains("\"task\""), "{list}");
    assert!(list.contains("\"dir\""), "{list}");
}

#[test]
fn help_and_missing_values_point_at_the_right_word() {
    let home = tempfile::tempdir().unwrap();
    let (code, help) = cron(home.path(), &[], &["add", "--help"]);
    assert_eq!(code, 0, "{help}");
    assert!(help.starts_with("usage:"), "{help}");
    cron(home.path(), &[], &["add", "x", "--every", "1h", "-p", "hi"]);
    let (code, text) = cron(home.path(), &[], &["log", "x", "--last"]);
    assert_ne!(code, 0);
    assert!(text.contains("--last needs a value"), "{text}");
    let (_, warned) = cron(
        home.path(),
        &[],
        &[
            "add",
            "y",
            "--every",
            "1h",
            "-p",
            "hi",
            "--mail",
            "nobody-here",
            "--chat",
        ],
    );
    assert!(warned.contains("nobody-here"), "{warned}");
    assert!(warned.contains("gateway setup telegram"), "{warned}");
}

#[test]
fn a_run_that_cannot_start_is_recorded_as_an_error() {
    let home = tempfile::tempdir().unwrap();
    let gone = home.path().join("gone");
    std::fs::create_dir_all(&gone).unwrap();
    let dir = gone.to_string_lossy().to_string();
    cron(
        home.path(),
        &[],
        &["add", "lost", "--every", "1h", "-p", "hi", "--dir", &dir],
    );
    std::fs::remove_dir_all(&gone).unwrap();
    cron(home.path(), &[], &["run", "lost"]);
    let (_, log) = cron(home.path(), &[], &["log", "lost"]);
    assert!(log.contains("error"), "{log}");
    assert!(!log.contains("running"), "{log}");
}

#[test]
fn the_short_endpoint_flag_works_in_cron() {
    let home = tempfile::tempdir().unwrap();
    let (code, text) = cron(
        home.path(),
        &[],
        &["add", "e", "--every", "1h", "-e", "zai", "-p", "hi"],
    );
    assert_eq!(code, 0, "{text}");
    let (_, list) = cron(home.path(), &[], &["list", "--json"]);
    assert!(list.contains("\"endpoint\":\"zai\""), "{list}");
}
