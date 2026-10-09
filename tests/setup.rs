use std::process::{Command, Stdio};

#[test]
fn a_first_run_without_a_provider_says_how_to_set_one_up() {
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(["-p", "hi"])
        .current_dir(home.path())
        .env_clear()
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("SUMMONR_HOME", home.path().join(".summonr"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(std::env::var_os("SYSTEMROOT").map(|root| ("SYSTEMROOT", root)))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("summonr: no provider is set up yet."),
        "{text}"
    );
}

#[test]
fn plan_mode_refuses_a_goal_or_steps_on_the_command_line() {
    let home = tempfile::tempdir().unwrap();
    for work in [["--goal", "fix it"], ["--steps", "PLAN.md"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
            .arg("--plan")
            .args(work)
            .current_dir(home.path())
            .env_clear()
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("SUMMONR_HOME", home.path().join(".summonr"))
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .envs(std::env::var_os("SYSTEMROOT").map(|root| ("SYSTEMROOT", root)))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{work:?}: {text}");
        assert!(text.contains("--plan denies writes"), "{work:?}: {text}");
    }
}

fn summonr(home: &std::path::Path, args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(args)
        .current_dir(home)
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("SUMMONR_HOME", home.join(".summonr"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(std::env::var_os("SYSTEMROOT").map(|root| ("SYSTEMROOT", root)))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), text)
}

#[test]
fn a_local_server_is_added_listed_first_and_removed_from_the_shell() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let (ok, text) = summonr(home, &["login", "nope"]);
    assert!(!ok && text.contains("--url URL"), "{text}");
    let (ok, text) = summonr(
        home,
        &[
            "login",
            "lab",
            "--url",
            "http://127.0.0.1:9/v1",
            "--model",
            "m",
        ],
    );
    assert!(ok && text.contains("Added lab"), "{text}");
    let (_, listed) = summonr(home, &["endpoints"]);
    assert!(listed.trim_start().starts_with("lab "), "{listed}");
    let (_, text) = summonr(home, &["logout", "lab"]);
    assert!(text.contains("needs no key"), "{text}");
    let (ok, text) = summonr(home, &["logout", "lab", "--remove"]);
    assert!(ok && text.contains("Removed the endpoint lab"), "{text}");
    let (_, listed) = summonr(home, &["endpoints"]);
    assert!(!listed.contains("lab "), "{listed}");
}

#[test]
fn a_local_server_updates_in_place_reads_ready_and_takes_its_default_with_it() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let url = "http://127.0.0.1:9/v1";
    let (ok, text) = summonr(home, &["login", "lab", "--url", url, "--model", "m"]);
    assert!(ok, "{text}");
    let (ok, text) = summonr(home, &["login", "lab", "--url", url, "--model", "m2"]);
    assert!(ok && text.contains("Updated"), "{text}");
    let (ok, text) = summonr(home, &["login", "lab"]);
    assert!(ok && text.contains("needs no key"), "{text}");
    let config = home.join(".summonr/config.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(
        &config,
        format!("default = \"lab\"\ndefault_model = \"m2\"\n{text}"),
    )
    .unwrap();
    let (ok, text) = summonr(home, &["logout", "lab", "--remove"]);
    assert!(ok, "{text}");
    let left = std::fs::read_to_string(&config).unwrap();
    assert!(!left.contains("default"), "{left}");
}

#[cfg(unix)]
#[test]
fn a_closed_pipe_ends_a_command_quietly() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let output = Command::new("sh")
        .args([
            "-c",
            &format!(
                "{{ sleep 0.3; exec '{}' login; }} | true",
                env!("CARGO_BIN_EXE_summonr")
            ),
        ])
        .current_dir(home)
        .env_clear()
        .env("HOME", home)
        .env("SUMMONR_HOME", home.join(".summonr"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!text.contains("panicked"), "{text}");
    let (_, next) = summonr(home, &["endpoints"]);
    assert!(!next.contains("without an exit record"), "{next}");
}
