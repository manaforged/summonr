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

fn run(home: &Path, args: &[&str]) -> (i32, String) {
    cron(home, &[], args)
}

#[test]
fn a_job_is_added_listed_and_removed() {
    let home = tempfile::tempdir().unwrap();
    let add = [
        "add",
        "nightly",
        "--cron",
        "0 3 * * *",
        "--goal",
        "make the tests pass",
    ];
    let (code, text) = run(
        home.path(),
        &[&add[..], &["--tz", "America/Chicago"][..]].concat(),
    );
    assert_eq!(code, 0, "{text}");
    let (_, list) = run(home.path(), &["list", "--json"]);
    let job: serde_json::Value = serde_json::from_str(list.trim()).unwrap();
    assert_eq!(job["name"], "nightly");
    assert_eq!(job["schedule"], "0 3 * * *");
    assert_eq!(job["paused"], false);
    let (code, text) = run(home.path(), &["remove", "nightly"]);
    assert_eq!(code, 0, "{text}");
    assert!(run(home.path(), &["list", "--json"]).1.is_empty());
}

#[test]
fn bad_adds_are_refused_with_the_reason() {
    let home = tempfile::tempdir().unwrap();
    for (args, want) in [
        (["x", "--cron", "61 * * * *", "-p", "hi"], "bad schedule"),
        (
            ["x", "--every", "1h", "--goal", "-rf everything"],
            "cannot start with `-`",
        ),
        (["old", "--at", "2020-01-01 00:00", "-p", "hi"], "past"),
        (
            ["bad", "--at", "yesterday", "-p", "hi"],
            "use HH:MM, YYYY-MM-DD HH:MM, or RFC 3339",
        ),
    ] {
        let (code, text) = run(home.path(), &[&["add"][..], &args[..]].concat());
        assert_ne!(code, 0, "{args:?}");
        assert!(text.contains(want), "{text}");
        assert!(!text.contains("strptime"), "{text}");
    }
    let (code, text) = run(home.path(), &["add", "soon", "--at", "23:59", "-p", "hi"]);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn a_run_still_going_skips_the_next_one() {
    let home = tempfile::tempdir().unwrap();
    let (code, text) = run(home.path(), &["add", "slow", "--every", "1h", "-p", "hi"]);
    assert_eq!(code, 0, "{text}");
    let runs = home.path().join(".summonr/cron/runs");
    std::fs::create_dir_all(&runs).unwrap();
    let held = std::fs::File::create(runs.join("slow.lock")).unwrap();
    held.lock().unwrap();
    let (code, text) = run(home.path(), &["exec", "slow", "--slot", "1"]);
    assert_eq!(code, 1, "{text}");
    let (_, log) = run(home.path(), &["log", "slow"]);
    assert!(log.contains("overlap"), "{log}");
    drop(held);
}

#[test]
fn cron_help_and_empty_states_guide_a_new_user() {
    let home = tempfile::tempdir().unwrap();
    for help in [&["--help"][..], &["add", "--help"][..]] {
        let (code, text) = run(home.path(), help);
        assert_eq!(code, 0, "{text}");
        assert!(text.starts_with("usage:"), "{text}");
    }
    let (code, list) = run(home.path(), &["list"]);
    assert_eq!(code, 0, "{list}");
    assert!(list.contains("no cron jobs; add one with"), "{list}");
    let (code, error) = run(home.path(), &["add", "x", "-p", "hi"]);
    assert_ne!(code, 0);
    assert!(error.contains("see `summonr cron --help`"), "{error}");
    assert!(!error.contains("usage:"), "{error}");
    run(home.path(), &["add", "x", "--every", "1h", "-p", "hi"]);
    let (code, text) = run(home.path(), &["log", "x", "--last"]);
    assert_ne!(code, 0);
    assert!(text.contains("--last needs a value"), "{text}");
    let mail = ["--mail", "nobody-here", "--chat"];
    let (_, warned) = run(
        home.path(),
        &[&["add", "y", "--every", "1h", "-p", "hi"][..], &mail[..]].concat(),
    );
    assert!(warned.contains("nobody-here"), "{warned}");
    assert!(warned.contains("gateway setup telegram"), "{warned}");
}

#[test]
fn a_job_reads_as_plain_lines() {
    let home = tempfile::tempdir().unwrap();
    let at = ["--at", "2026-12-01 09:00", "--tz", "America/Chicago"];
    let (code, added) = run(
        home.path(),
        &[&["add", "Nightly", "--goal", "ship it"][..], &at[..]].concat(),
    );
    assert_eq!(code, 0, "{added}");
    assert!(
        added.starts_with("added nightly: at 2026-12-01 09:00 CST, next"),
        "{added}"
    );
    let (_, show) = run(home.path(), &["show", "nightly"]);
    assert!(show.contains("goal: ship it"), "{show}");
    assert!(show.contains("budget: 30m00s (default)"), "{show}");
    assert!(!show.contains("created = "), "{show}");
    let (_, log) = run(home.path(), &["log", "nightly"]);
    assert_eq!(log.trim(), "no runs yet");
    let (_, token) = run(home.path(), &["token", "nightly"]);
    let secret = token.lines().next().unwrap_or_default();
    let hint = token.replace(secret, "<token>");
    assert!(hint.contains("Authorization: Bearer $TOKEN"), "{hint}");
    assert!(hint.contains("/run/nightly"), "{hint}");
    assert_eq!(
        token.matches(secret).count(),
        1,
        "the token is printed once"
    );
    let (_, bad) = run(
        home.path(),
        &["add", "y", "--every", "1h", "-p", "hi", "--notify", "loud"],
    );
    assert!(!bad.contains("unknown variant"), "{bad}");
}

#[test]
fn a_cron_run_cannot_change_jobs_even_through_run_command() {
    let home = tempfile::tempdir().unwrap();
    run(home.path(), &["add", "job", "--every", "1h", "-p", "hi"]);
    let more = ["add", "more", "--every", "1m", "-p", "hi"];
    for words in [
        &more[..],
        &["token", "job"][..],
        &["resume", "job"][..],
        &["run", "job"][..],
    ] {
        let (code, text) = cron(home.path(), &[("SUMMONR_CRON", "job")], words);
        assert_ne!(code, 0, "{words:?}: {text}");
        assert!(
            text.contains("a cron run cannot change cron jobs"),
            "{text}"
        );
    }
    assert!(!run(home.path(), &["list", "--json"]).1.contains("more"));
    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args([
            "tool",
            "run_command",
            if cfg!(windows) {
                r#"{"command":"echo cron=$env:SUMMONR_CRON"}"#
            } else {
                r#"{"command":"echo cron=$SUMMONR_CRON"}"#
            },
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
fn manual_jobs_never_fire_on_their_own_and_runs_read_as_answers() {
    let home = tempfile::tempdir().unwrap();
    let (code, text) = run(home.path(), &["add", "alert", "--manual", "-p", "triage"]);
    assert_eq!(code, 0, "{text}");
    run(home.path(), &["tick"]);
    let (_, log) = run(home.path(), &["log", "alert"]);
    assert!(log.contains("no runs yet"), "{log}");
    run(home.path(), &["run", "alert"]);
    let (_, log) = run(home.path(), &["log", "alert"]);
    assert!(!log.contains("{\"type\""), "{log}");
    assert!(log.contains("log: "), "{log}");
    run(
        home.path(),
        &["add", "e", "--every", "1h", "-e", "zai", "-p", "hi"],
    );
    let (_, list) = run(home.path(), &["list", "--json"]);
    for field in [
        "\"paused\":false",
        "\"task\"",
        "\"dir\"",
        "\"endpoint\":\"zai\"",
    ] {
        assert!(list.contains(field), "{list}");
    }
}

#[test]
fn a_run_that_cannot_start_is_recorded_as_an_error() {
    let home = tempfile::tempdir().unwrap();
    let gone = home.path().join("gone");
    std::fs::create_dir_all(&gone).unwrap();
    let dir = gone.to_string_lossy().to_string();
    run(
        home.path(),
        &["add", "lost", "--every", "1h", "-p", "hi", "--dir", &dir],
    );
    std::fs::remove_dir_all(&gone).unwrap();
    run(home.path(), &["run", "lost"]);
    let (_, log) = run(home.path(), &["log", "lost"]);
    assert!(log.contains("error"), "{log}");
    assert!(!log.contains("running"), "{log}");
}

#[test]
fn a_job_names_the_endpoint_it_runs_on_and_unknown_jobs_are_not_config_errors() {
    let home = tempfile::tempdir().unwrap();
    let (_, added) = run(home.path(), &["add", "bare", "--every", "1h", "-p", "hi"]);
    assert!(added.contains("endpoint none set up"), "{added}");
    std::fs::create_dir_all(home.path().join(".summonr")).unwrap();
    std::fs::write(
        home.path().join(".summonr/config.toml"),
        "[endpoint.local]\nbase_url = \"http://127.0.0.1:9/v1\"\nmodel = \"m\"\napi_key = \"k\"\n",
    )
    .unwrap();
    let (_, show) = run(home.path(), &["show", "bare"]);
    assert!(show.contains("endpoint: local ("), "{show}");
    let (_, added) = run(
        home.path(),
        &["add", "pinned", "--every", "1h", "-p", "hi", "-e", "local"],
    );
    assert!(
        added.contains("endpoint local\n") || added.ends_with("endpoint local"),
        "{added}"
    );
    let (code, missing) = run(home.path(), &["pause", "nosuch"]);
    assert_ne!(code, 0);
    assert!(missing.contains("no cron job named `nosuch`"), "{missing}");
    assert!(!missing.contains("configuration error"), "{missing}");
}

#[test]
fn a_wrong_cron_flag_is_a_usage_error() {
    let home = tempfile::tempdir().unwrap();
    let (code, text) = run(home.path(), &["run", "nosuch", "--loud"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("unknown option `--loud`"), "{text}");
    assert!(!text.contains("configuration error"), "{text}");
}

#[test]
fn cron_run_takes_notify_for_this_run() {
    let home = tempfile::tempdir().unwrap();
    let (code, text) = run(home.path(), &["run", "nosuch", "--notify", "never"]);
    assert_eq!(code, 1, "{text}");
    assert!(!text.contains("unknown option"), "{text}");
    assert!(text.contains("nosuch"), "{text}");
}

#[test]
fn cron_add_sees_a_key_stored_for_a_route_name() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".summonr")).unwrap();
    std::fs::write(
        home.path().join(".summonr/credentials.toml"),
        "zai-intl = \"fixture-key\"\n",
    )
    .unwrap();
    let env = [("SUMMONR_SECRETS", "file")];
    let args = |name, endpoint| ["add", name, "--manual", "-p", "hi", "-e", endpoint];
    let (_, keyed) = cron(home.path(), &env, &args("keyed", "zai-intl"));
    assert!(keyed.contains("added keyed"), "{keyed}");
    assert!(!keyed.contains("no usable key"), "{keyed}");
    let (_, bare) = cron(home.path(), &env, &args("bare", "deepseek"));
    assert!(bare.contains("deepseek has no usable key"), "{bare}");
    let shell = [
        ("SUMMONR_SECRETS", "file"),
        ("DEEPSEEK_API_KEY", "fixture-key"),
    ];
    let (_, from_shell) = cron(home.path(), &shell, &args("shell", "deepseek"));
    assert!(
        from_shell.contains("deepseek has no usable key"),
        "a key only in the shell does not reach cron runs: {from_shell}"
    );
}
