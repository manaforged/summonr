use std::path::Path;
use std::process::{Command, Output, Stdio};

#[test]
fn a_first_run_without_a_provider_says_how_to_set_one_up() {
    let home = tempfile::tempdir().unwrap();
    let output = run(home.path(), &["-p", "hi"]);
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{text}");
    assert!(
        text.contains("summonr: no provider is set up yet."),
        "{text}"
    );
}

#[test]
fn there_is_no_plan_mode_flag() {
    let home = tempfile::tempdir().unwrap();
    for work in [["--plan", "-p", "hi"], ["--plan", "--goal", "fix it"]] {
        let output = run(home.path(), &work);
        let text = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{work:?}: {text}");
        assert!(text.contains("unknown option `--plan`"), "{work:?}: {text}");
    }
}

#[test]
fn invalid_cli_arguments_fail_before_starting_a_session() {
    let home = tempfile::tempdir().expect("isolated CLI home");
    for (args, expected) in [
        (vec!["modules", "extra"], "takes no arguments"),
        (vec!["--goal", "fix", "ignored"], "takes no prompt"),
        (vec!["--max-tokens", "0", "hi"], "positive integer"),
        (vec!["login", "openai", "--model", "m"], "--default --model"),
        (
            vec![
                "mcp",
                "add",
                "fixture",
                "--url",
                "https://example.com",
                "--env",
                "A=B",
            ],
            "use `--header`",
        ),
    ] {
        let (ok, text) = summonr(home.path(), &args);
        assert!(!ok && text.contains(expected), "{args:?}: {text}");
    }
    assert!(!home.path().join(".summonr/sessions").exists());
}

#[test]
fn global_json_reaches_mcp_list_and_worktree_prune() {
    let home = tempfile::tempdir().expect("isolated CLI home");
    let (ok, text) = summonr(home.path(), &["--json", "mcp", "list"]);
    assert!(ok, "{text}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(text.trim()).expect("MCP JSON"),
        serde_json::json!([])
    );
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(home.path())
        .status()
        .expect("fixture repository");
    assert!(status.success());
    let (ok, text) = summonr(home.path(), &["worktrees", "prune", "--json"]);
    assert!(ok, "{text}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(text.trim()).expect("prune JSON"),
        serde_json::json!([])
    );
}

fn isolated(program: &str, home: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(home)
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("SUMMONR_HOME", home.join(".summonr"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(std::env::var_os("SYSTEMROOT").map(|root| ("SYSTEMROOT", root)));
    command
}

fn run(home: &Path, args: &[&str]) -> Output {
    isolated(env!("CARGO_BIN_EXE_summonr"), home)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn summonr(home: &Path, args: &[&str]) -> (bool, String) {
    let output = run(home, args);
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
    let output = isolated("sh", home)
        .args([
            "-c",
            &format!(
                "{{ sleep 0.3; exec '{}' login; }} | true",
                env!("CARGO_BIN_EXE_summonr")
            ),
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!text.contains("panicked"), "{text}");
    let (_, next) = summonr(home, &["endpoints"]);
    assert!(!next.contains("without an exit record"), "{next}");
}

#[test]
fn a_local_server_takes_a_new_model_and_becomes_the_default_from_the_shell() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let url = "http://127.0.0.1:9/v1";
    let (ok, text) = summonr(home, &["login", "lab", "--url", url, "--model", "m"]);
    assert!(ok, "{text}");
    let (ok, text) = summonr(home, &["login", "lab", "--model", "m2"]);
    assert!(ok && text.contains("m2"), "{text}");
    let (ok, text) = summonr(home, &["login", "lab", "--default"]);
    assert!(ok, "{text}");
    let config = std::fs::read_to_string(home.join(".summonr/config.toml")).unwrap();
    assert!(
        config.contains("default = \"lab\"") && config.contains("model = \"m2\""),
        "{config}"
    );
    let (_, help) = summonr(home, &["login", "--help"]);
    assert!(
        help.contains("--url") && help.contains("--default"),
        "{help}"
    );
}

#[test]
fn acp_answers_initialize_before_a_provider_is_set_up() {
    use std::io::Write;
    let home = tempfile::tempdir().unwrap();
    let mut child = isolated(env!("CARGO_BIN_EXE_summonr"), home.path())
        .arg("acp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let cwd = home.path().display().to_string().replace('\\', "\\\\");
    let requests = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":1}}}}\n{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"session/new\",\"params\":{{\"cwd\":\"{cwd}\",\"mcpServers\":[]}}}}\n"
    );
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(requests.as_bytes()).unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let replies: Vec<serde_json::Value> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let reply = |id: i64| replies.iter().find(|reply| reply["id"] == id);
    assert!(
        reply(1).is_some_and(|reply| reply["result"]["protocolVersion"] == 1),
        "{replies:?}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        reply(2).is_some_and(|reply| reply["error"]["message"]
            .as_str()
            .is_some_and(|text| text.contains("no provider"))),
        "{replies:?}"
    );
}

#[test]
fn the_tool_command_runs_a_module_tool_that_is_switched_on() {
    let home = tempfile::tempdir().unwrap();
    let summonr_home = home.path().join(".summonr");
    std::fs::create_dir_all(&summonr_home).unwrap();
    std::fs::write(
        summonr_home.join("config.toml"),
        "[modules]\nbrowser = true\n",
    )
    .unwrap();
    let (_, text) = summonr(home.path(), &["tool", "browser", "{}"]);
    assert!(!text.contains("unavailable in the current mode"), "{text}");
}

#[cfg(unix)]
#[test]
fn a_child_that_exits_before_reading_does_not_end_summonr() {
    let home = tempfile::tempdir().unwrap();
    for index in 0..20 {
        let output = run(
            home.path(),
            &["mcp", "add", &format!("demo{index}"), "--", "/usr/bin/true"],
        );
        let text = String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "run {index}: {text:?}");
        assert!(text.contains("not reachable"), "run {index}: {text:?}");
    }
}
