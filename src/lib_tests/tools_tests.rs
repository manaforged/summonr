use super::*;

#[test]
fn sandbox_command_wraps_the_existing_payload() -> Result<()> {
    let workspace = test_workspace()?;
    let Some(sandbox) = sandbox_for_test(&workspace)? else {
        std::fs::remove_dir_all(workspace)?;
        return Ok(());
    };
    let (_, args) = sandbox.command_line("sh", &["-c", "printf ok"], &workspace);
    let args: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let workspace_path = workspace.canonicalize()?.display().to_string();
    assert!(args.windows(2).any(|pair| pair == ["-c", "printf ok"]));
    if cfg!(target_os = "macos") {
        assert_eq!(args.first().map(String::as_str), Some("-p"));
        assert!(
            args.get(1)
                .is_some_and(|profile| profile.contains(&workspace_path))
        );
        let temp_path = std::env::temp_dir().canonicalize()?.display().to_string();
        let profile = args
            .get(1)
            .ok_or_else(|| Error::Tool("missing Seatbelt profile".to_string()))?;
        assert!(profile.contains("(allow default)"));
        assert!(profile.contains("(deny file-write*)"));
        assert!(profile.contains(&temp_path));
    } else if cfg!(target_os = "linux") {
        let temp_path = std::env::temp_dir().canonicalize()?.display().to_string();
        assert!(args.windows(2).any(|pair| pair == ["--ro-bind", "/"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--bind", workspace_path.as_str()])
        );
        assert!(
            args.windows(3)
                .any(|window| { window == ["--bind", temp_path.as_str(), temp_path.as_str()] })
        );
        assert!(args.contains(&"--chdir".to_string()));
    }
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn edit_file_replaces_by_tag_and_rejects_stale_hash() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let read = tools
        .execute(&call("read_file", r#"{"path":"note.txt"}"#))
        .await?;
    assert!(read.contains(&format!("1:{}|hello", line_hash("hello"))));
    let tag = format!("1:{}", line_hash("hello"));
    let edited = tools
        .execute(&call(
            "edit_file",
            &format!(r#"{{"path":"note.txt","replace":{{"tag":"{tag}","new":"hi"}}}}"#),
        ))
        .await?;
    assert!(edited.contains(&format!("1:{}|hi", line_hash("hi"))));
    let stale = tools
        .execute(&call(
            "edit_file",
            &format!(r#"{{"path":"note.txt","replace":{{"tag":"{tag}","new":"no"}}}}"#),
        ))
        .await;
    stale.unwrap_err();
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn read_file_rejects_paths_outside_workspace() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let result = tools
        .execute(&call("read_file", r#"{"path":"../outside.txt"}"#))
        .await;
    result.unwrap_err();
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn write_file_creates_and_hash_edit_survives_insert() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    tools
        .execute(&call(
            "write_file",
            r#"{"path":"src/n.rs","content":"fn a() {}\nfn b() {}\n"}"#,
        ))
        .await?;
    let body = std::fs::read_to_string(workspace.join("src/n.rs"))?;
    assert_eq!(body, "fn a() {}\nfn b() {}\n");
    let hash_b = line_hash("fn b() {}");
    tools
        .execute(&call(
            "edit_file",
            &format!(
                r#"{{"path":"src/n.rs","insert_after":{{"tag":"{}","text":"fn mid() {{}}"}}}}"#,
                line_hash("fn a() {}")
            ),
        ))
        .await?;
    let edited = tools
        .execute(&call(
            "edit_file",
            &format!(r#"{{"path":"src/n.rs","replace":{{"tag":"{hash_b}","new":"fn c() {{}}"}}}}"#),
        ))
        .await?;
    assert!(edited.contains("fn c() {}"));
    assert_eq!(
        std::fs::read_to_string(workspace.join("src/n.rs"))?,
        "fn a() {}\nfn mid() {}\nfn c() {}\n"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn grep_returns_hashline_hits() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let out = tools
        .execute(&call("grep", r#"{"pattern":"hello"}"#))
        .await?;
    assert!(out.contains(&format!("note.txt:1:{}|hello", line_hash("hello"))));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn grep_accepts_a_file_path() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let out = tools
        .execute(&call("grep", r#"{"path":"note.txt","pattern":"hello"}"#))
        .await?;
    assert!(out.contains(&format!("note.txt:1:{}|hello", line_hash("hello"))));
    assert!(out.contains("(1 files)"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn grep_glob_filters_files() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::write(workspace.join("other.rs"), "hello from rust\n")?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let rust = tools
        .execute(&call("grep", r#"{"pattern":"hello","glob":"*.rs"}"#))
        .await?;
    assert!(rust.contains("other.rs"), "{rust}");
    assert!(!rust.contains("note.txt"), "{rust}");
    let listed = tools
        .execute(&call("grep", r#"{"pattern":"","glob":"*.txt"}"#))
        .await?;
    assert!(listed.contains("note.txt"), "{listed}");
    assert!(!listed.contains("other.rs"), "{listed}");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn edit_batch_resolves_every_anchor_against_the_original_file() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::write(workspace.join("batch.txt"), "one\ntwo\nthree\nfour\n")?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    tools
        .execute(&call(
            "edit_file",
            &format!(
                r#"{{"path":"batch.txt","edits":[{{"delete":{{"tag":"1:{}"}}}},{{"replace":{{"tag":"4:{}","new":"FOUR"}}}}]}}"#,
                line_hash("one"),
                line_hash("four")
            ),
        ))
        .await?;
    assert_eq!(
        std::fs::read_to_string(workspace.join("batch.txt"))?,
        "two\nthree\nFOUR\n"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn edit_batch_accepts_adjacent_anchors_without_losing_insertions() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::write(workspace.join("adjacent.txt"), "one\ntwo\nthree\n")?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let one = line_hash("one");
    let two = line_hash("two");
    tools
        .execute(&call(
            "edit_file",
            &format!(
                r#"{{"path":"adjacent.txt","edits":[{{"replace":{{"tag":"{two}","new":"TWO"}}}},{{"insert_after":{{"tag":"{one}","text":"between"}}}}]}}"#
            ),
        ))
        .await?;
    assert_eq!(
        std::fs::read_to_string(workspace.join("adjacent.txt"))?,
        "one\nbetween\nTWO\nthree\n"
    );

    std::fs::write(workspace.join("overlap.txt"), "one\ntwo\n")?;
    let before = std::fs::read_to_string(workspace.join("overlap.txt"))?;
    let error = tools
        .execute(&call(
            "edit_file",
            &format!(
                r#"{{"path":"overlap.txt","edits":[{{"delete":{{"tag":"{one}"}}}},{{"insert_after":{{"tag":"{one}","text":"replacement"}}}}]}}"#
            ),
        ))
        .await;
    error.unwrap_err();
    assert_eq!(
        std::fs::read_to_string(workspace.join("overlap.txt"))?,
        before
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn run_command_echoes_and_force_push_is_denied() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let out = tools
        .execute(&call("run_command", r#"{"command":"echo hi"}"#))
        .await?;
    assert!(out.contains("exit 0"));
    assert!(out.contains("hi"));
    let denied = tools
        .execute(&call(
            "run_command",
            r#"{"command":"git push --force origin main"}"#,
        ))
        .await;
    denied.unwrap_err();
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn run_command_preserves_a_failed_pipeline_status() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let outcome = tools
        .execute_outcome(&call(
            "run_command",
            r#"{"command":"printf failure | grep failure; false | true"}"#,
        ))
        .await?;
    assert!(!outcome.success);
    assert!(outcome.text.contains("exit 1"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn run_command_sees_the_seat_env() -> Result<()> {
    let workspace = test_workspace()?;
    let mut tools = crate::tools::ToolRegistry::new(&workspace)?;
    tools.set_env([("MAGE_SEAT_VAR".to_string(), "clean-toolchain".to_string())]);
    let out = tools
        .execute(&call("run_command", r#"{"command":"echo $MAGE_SEAT_VAR"}"#))
        .await?;
    assert!(out.contains("clean-toolchain"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn sandbox_contains_foreground_and_background_writes() -> Result<()> {
    let workspace = test_workspace()?;
    let Some(_) = sandbox_for_test(&workspace)? else {
        std::fs::remove_dir_all(workspace)?;
        return Ok(());
    };
    let mut tools = crate::tools::ToolRegistry::new(&workspace)?;
    tools.enable_sandbox(false)?;
    let outside = outside_fixture(&workspace, "foreground");
    let outside_background = outside_fixture(&workspace, "background");
    assert!(!outside.exists());
    assert!(!outside_background.exists());

    tools
        .execute(&call(
            "run_command",
            &format!(
                r#"{{"command":"printf inside > {}"}}"#,
                shell_arg(std::path::Path::new("inside.txt"))
            ),
        ))
        .await?;
    assert_eq!(
        std::fs::read_to_string(workspace.join("inside.txt"))?,
        "inside"
    );

    let blocked = tools
        .execute(&call(
            "run_command",
            &format!(r#"{{"command":"touch {}"}}"#, shell_arg(&outside)),
        ))
        .await?;
    assert!(blocked.starts_with("exit "), "{blocked}");
    assert!(!blocked.starts_with("exit 0"), "{blocked}");
    assert!(!outside.exists());

    let started = tools
        .execute(&call(
            "run_command",
            &format!(
                r#"{{"command":"printf background > {}","background":true}}"#,
                shell_arg(std::path::Path::new("background.txt"))
            ),
        ))
        .await?;
    let id = started
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .ok_or_else(|| Error::Tool("missing sandbox job id".to_string()))?;
    let poll = format!(r#"{{"job":"{id}"}}"#);
    let mut state = String::new();
    for _ in 0..20 {
        state = tools.execute(&call("run_command", &poll)).await?;
        if state.contains("state: exit") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(state.contains("state: exit 0"), "{state}");
    assert_eq!(
        std::fs::read_to_string(workspace.join("background.txt"))?,
        "background"
    );

    let started = tools
        .execute(&call(
            "run_command",
            &format!(
                r#"{{"command":"touch {}","background":true}}"#,
                shell_arg(&outside_background)
            ),
        ))
        .await?;
    let id = started
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .ok_or_else(|| Error::Tool("missing sandbox background job id".to_string()))?;
    let poll = format!(r#"{{"job":"{id}"}}"#);
    let mut state = String::new();
    for _ in 0..20 {
        state = tools.execute(&call("run_command", &poll)).await?;
        if state.contains("state: exit") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(state.contains("state: exit"), "{state}");
    assert!(!state.contains("state: exit 0"), "{state}");
    assert!(!outside_background.exists());

    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn sandbox_no_net_blocks_a_real_connection() -> Result<()> {
    let workspace = test_workspace()?;
    let Some(allowed) = sandbox_for_test_with(&workspace, false)? else {
        std::fs::remove_dir_all(workspace)?;
        return Ok(());
    };
    let Some(denied) = sandbox_for_test_with(&workspace, true)? else {
        std::fs::remove_dir_all(workspace)?;
        return Ok(());
    };
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let script = format!(
        "import socket; socket.create_connection(('127.0.0.1', {port}), timeout=2).close()"
    );

    let allowed_output = allowed
        .command("/usr/bin/env", &["python3", "-c", &script], &workspace)
        .output()
        .await?;
    assert!(
        allowed_output.status.success(),
        "network control failed: {}",
        String::from_utf8_lossy(&allowed_output.stderr)
    );
    let _ = listener.accept()?;

    let denied_output = denied
        .command("/usr/bin/env", &["python3", "-c", &script], &workspace)
        .output()
        .await?;
    assert!(!denied_output.status.success());
    listener.set_nonblocking(true)?;
    let error = listener
        .accept()
        .err()
        .ok_or_else(|| Error::Tool("the denied connection reached the listener".to_string()))?;
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn repeated_command_is_not_rerun_until_something_changes() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let first = tools
        .execute(&call("run_command", r#"{"command":"echo once"}"#))
        .await?;
    let second = tools
        .execute(&call("run_command", r#"{"command":"echo once"}"#))
        .await?;
    assert!(!first.contains("result repeated"));
    assert!(second.contains("result repeated"), "{second}");
    assert!(second.contains("once"));
    tools
        .execute(&call("write_file", r#"{"path":"touch.txt","content":"x"}"#))
        .await?;
    let third = tools
        .execute(&call("run_command", r#"{"command":"echo once"}"#))
        .await?;
    assert!(
        !third.contains("result repeated"),
        "an edit invalidates the repeat"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[test]
fn digest_keeps_errors_failures_and_summaries() {
    let mut log = String::new();
    for i in 0..120 {
        log.push_str(&format!("   Compiling dep{i} v0.1.0\n"));
    }
    log.push_str("error[E0308]: mismatched types\n  --> src/main.rs:4:5\n   |\n 4 |     let x: u32 = \"a\";\n   |                  ^^^ expected `u32`\n\n");
    for i in 0..40 {
        log.push_str(&format!("test m::t{i} ... ok\n"));
    }
    log.push_str("test m::bad ... FAILED\n\nfailures:\n\n---- m::bad stdout ----\nthread 'm::bad' panicked at src/m.rs:9:5:\nboom\n\ntest result: FAILED. 40 passed; 1 failed\n");
    let out = crate::tools::digest("cargo test 2>&1", &log);
    assert!(out.contains("error[E0308]"));
    assert!(out.contains("expected `u32`"));
    assert!(out.contains("m::bad ... FAILED"));
    assert!(out.contains("panicked at src/m.rs:9:5"));
    assert!(out.contains("test result: FAILED"));
    assert!(!out.contains("Compiling dep5 "), "noise dropped");
    assert!(!out.contains("test m::t7 ... ok"), "passing tests dropped");
    let short = crate::tools::digest("cargo build", "Finished dev\n");
    assert_eq!(short, "Finished dev");
    let plain: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let tail = crate::tools::digest("ls -R", &plain);
    assert!(tail.starts_with("line 0\n"));
    assert!(tail.contains("[... 140 lines captured ...]"));
    assert!(tail.ends_with("line 199"));
}

#[tokio::test]
async fn edit_file_applies_several_edits_in_one_call() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(workspace.join("f.txt"), "alpha\nbeta\ngamma\n")?;
    let a = crate::hashline::line_hash("alpha");
    let c = crate::hashline::line_hash("gamma");
    let args = format!(
        r#"{{"path":"f.txt","edits":[{{"replace":{{"tag":"{a}","new":"ALPHA"}}}},{{"insert_after":{{"tag":"{c}","text":"delta"}}}}]}}"#
    );
    let out = tools.execute(&call("edit_file", &args)).await?;
    assert!(out.starts_with("edited f.txt"));
    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "ALPHA\nbeta\ngamma\ndelta\n"
    );
    let bad = format!(r#"{{"path":"f.txt","edits":[{{"replace":{{"tag":"{a}","new":"x"}}}}]}}"#);
    let err = tools.execute(&call("edit_file", &bad)).await;
    assert!(err.is_err(), "stale tag in a batch fails before any write");
    tools
        .execute(&call("edit_file", r#"{"path":"f.txt","edits":[]}"#))
        .await
        .unwrap_err();
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn edit_tags_resolve_repeats_and_explain_misses() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(
        workspace.join("f.txt"),
        "fn a() {\n    1\n}\nfn b() {\n    2\n}\n",
    )?;
    let start = crate::hashline::line_hash("fn a() {");
    let close = crate::hashline::line_hash("}");
    let args = format!(
        r#"{{"path":"f.txt","replace":{{"tag":"{start}","end_tag":"{close}","new":"fn a() {{ 0 }}"}}}}"#
    );
    tools.execute(&call("edit_file", &args)).await?;
    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "fn a() { 0 }\nfn b() {\n    2\n}\n"
    );

    std::fs::write(workspace.join("g.txt"), "x\ny\nx\n")?;
    let x = crate::hashline::line_hash("x");
    let err = tools
        .execute(&call(
            "edit_file",
            &format!(r#"{{"path":"g.txt","replace":{{"tag":"{x}","new":"z"}}}}"#),
        ))
        .await
        .map(|_| String::new())
        .unwrap_or_else(|error| error.to_string());
    assert!(err.contains("matches 2 lines"), "{err}");
    assert!(err.contains(&format!("1:{x}|x")) && err.contains(&format!("3:{x}|x")));

    let err = tools
        .execute(&call(
            "edit_file",
            r#"{"path":"g.txt","replace":{"tag":"2:0000","new":"z"}}"#,
        ))
        .await
        .map(|_| String::new())
        .unwrap_or_else(|error| error.to_string());
    assert!(err.contains("stale tag `2:0000`"), "{err}");
    assert!(err.contains("|y"), "{err}");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn edit_file_accepts_near_miss_shapes() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(workspace.join("f.txt"), "one\ntwo\n")?;
    let one = crate::hashline::line_hash("one");
    let two = crate::hashline::line_hash("two");
    tools
        .execute(&call(
            "edit_file",
            &format!(
                r#"{{"path":"f.txt","edits":[{{"tag":"{one}","new":"ONE"}},{{"replace":{{"tag":"{two}","text":"TWO"}}}}]}}"#
            ),
        ))
        .await?;
    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "ONE\nTWO\n"
    );
    let one = crate::hashline::line_hash("ONE");
    let two = crate::hashline::line_hash("TWO");
    tools
        .execute(&call(
            "edit_file",
            &format!(
                r#"{{"path":"f.txt","edits":[{{"tag":"{one}","text":"between"}},{{"op":"delete","tag":"{two}"}}]}}"#
            ),
        ))
        .await?;
    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "ONE\nbetween\n"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn identical_calls_get_a_loop_warning_on_the_third() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let probe = || call("read_file", r#"{"path":"missing.txt"}"#);
    let first = tools
        .execute(&probe())
        .await
        .map(|_| String::new())
        .unwrap_or_else(|error| error.to_string());
    drop(tools.execute(&probe()).await);
    let third = tools
        .execute(&probe())
        .await
        .map(|_| String::new())
        .unwrap_or_else(|error| error.to_string());
    assert!(!first.contains("identical calls"));
    assert!(third.contains("3 identical calls in a row"), "{third}");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn writes_return_advisory_file_checks() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let bad_outcome = tools
        .execute_outcome(&call(
            "write_file",
            r#"{"path":"also-bad.js","content":"function ( {\n"}"#,
        ))
        .await?;
    assert!(
        bad_outcome.success,
        "the file was written despite the diagnostic"
    );
    assert!(
        !bad_outcome.text.contains("check: ok (node)"),
        "{}",
        bad_outcome.text
    );
    let plain = tools
        .execute(&call(
            "write_file",
            r#"{"path":"notes.txt","content":"hi\n"}"#,
        ))
        .await?;
    assert!(!plain.contains("check: ok (node)"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn rust_writes_do_not_run_a_workspace_build() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::create_dir_all(workspace.join("src"))?;
    std::fs::write(
        workspace.join("Cargo.toml"),
        "[package]\nname = \"no-edit-build\"\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n",
    )?;
    std::fs::write(
        workspace.join("build.rs"),
        "fn main() { std::fs::write(\"build-ran\", \"yes\").unwrap(); }\n",
    )?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;

    let result = tools
        .execute(&call(
            "write_file",
            r#"{"path":"src/lib.rs","content":"pub fn value() -> u8 { 1 }\n"}"#,
        ))
        .await?;

    assert!(!result.contains("check: ok (node)"), "{result}");
    assert!(!workspace.join("build-ran").exists());
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn sandbox_wraps_post_edit_checks() -> Result<()> {
    let workspace = test_workspace()?;
    let Some(_) = sandbox_for_test(&workspace)? else {
        std::fs::remove_dir_all(workspace)?;
        return Ok(());
    };
    let mut tools = crate::tools::ToolRegistry::new(&workspace)?;
    tools.enable_sandbox(false)?;
    let result = tools
        .execute(&call(
            "write_file",
            r#"{"path":"sandbox.js","content":"export const value = 1;\n"}"#,
        ))
        .await?;
    assert!(!result.contains("check: ok (node)"), "{result}");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn run_command_reports_nonzero_exit() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let out = tools
        .execute(&call("run_command", r#"{"command":"exit 3"}"#))
        .await?;
    assert!(out.starts_with("exit 3"), "{out}");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn nonzero_command_emits_a_failed_tool_result() -> Result<()> {
    struct ExitThenText(AtomicUsize);
    impl Provider for ExitThenText {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                match self.0.fetch_add(1, Ordering::SeqCst) {
                    0 => Ok(AssistantMessage {
                        tool_calls: vec![ToolCall {
                            id: "exit".to_string(),
                            kind: ToolKind::Function,
                            function: ToolFunction {
                                name: "run_command".to_string(),
                                arguments: r#"{"command":"exit 3"}"#.to_string(),
                            },
                        }],
                        ..AssistantMessage::default()
                    }),
                    1 => {
                        assert!(request.messages.last().is_some_and(|message| {
                            message.role == crate::provider::Role::Tool
                                && message.content.starts_with("exit 3")
                        }));
                        Ok(AssistantMessage {
                            tool_calls: vec![call("task_state", r#"{"action":"advance"}"#)],
                            ..AssistantMessage::default()
                        })
                    }
                    _ => Ok(AssistantMessage {
                        content: Some("handled".to_string()),
                        ..AssistantMessage::default()
                    }),
                }
            })
        }
    }

    let workspace = test_workspace()?;
    let (task, _) = TaskState::transition(
        None,
        r#"{"action":"start","steps":["Run the failing command"]}"#,
    )?;
    let mut loop_ = Loop::with_session(
        ExitThenText(AtomicUsize::new(0)),
        "test-model",
        &workspace,
        "test",
    )?
    .restore_replay(Replay {
        task: Some(task),
        ..Replay::default()
    });
    let turn = loop_.turn("run it").await?;
    assert!(turn.events.iter().any(|event| matches!(
        event,
        Event::ToolResult {
            id,
            success: false,
            output,
            ..
        } if id == "exit" && output.starts_with("exit 3")
    )));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn run_command_times_out() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let result = tools
        .execute(&call(
            "run_command",
            r#"{"command":"sleep 5","timeout_secs":1}"#,
        ))
        .await;
    result.unwrap_err();
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn run_command_streams_large_output_to_a_job_log() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let out = tools
        .execute(&call(
            "run_command",
            r#"{"command":"awk 'BEGIN{for(i=0;i<8000;i++) print \"line-\" i}'"}"#,
        ))
        .await?;
    assert!(out.contains("exit 0"));
    assert!(out.contains("line-0"));
    assert!(out.contains("line-7999"));
    assert!(!out.contains(".mage/jobs/"));
    assert!(out.contains("lines captured"));
    assert!(workspace.join(".mage/jobs").read_dir()?.next().is_some());
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn run_command_background_job_reaches_exit() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let started = tools
        .execute(&call(
            "run_command",
            r#"{"command":"sleep 1; echo done-bg","background":true}"#,
        ))
        .await?;
    assert!(started.contains("started job"));
    assert!(started.contains("state: running"));
    let id = started
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .ok_or_else(|| Error::Tool("missing job id".to_string()))?;
    let poll = format!(r#"{{"job":"{id}"}}"#);
    let mut state = String::new();
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        state = tools.execute(&call("run_command", &poll)).await?;
        if state.contains("state: exit 0") {
            break;
        }
    }
    assert!(state.contains("state: exit 0"), "{state}");
    assert!(state.contains("done-bg"), "{state}");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn failed_background_job_poll_is_a_failed_tool_result() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let started = tools
        .execute(&call(
            "run_command",
            r#"{"command":"exit 7","background":true}"#,
        ))
        .await?;
    let id = started
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .ok_or_else(|| Error::Tool("missing job id".to_string()))?;
    let mut outcome = None;
    for _ in 0..100 {
        let current = tools
            .execute_outcome(&call(
                "run_command",
                &serde_json::json!({ "job": id }).to_string(),
            ))
            .await?;
        let finished = current.text.contains("state: exit");
        outcome = Some(current);
        if finished {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let outcome = outcome.ok_or_else(|| Error::Tool("job did not finish".to_string()))?;
    assert!(!outcome.success);
    assert!(outcome.text.contains("state: exit 7"), "{}", outcome.text);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn run_command_kill_stops_a_background_job() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let started = tools
        .execute(&call(
            "run_command",
            r#"{"command":"sleep 30","background":true}"#,
        ))
        .await?;
    let id = started
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .ok_or_else(|| Error::Tool("missing job id".to_string()))?;
    let poll = format!(r#"{{"job":"{id}","kill":true}}"#);
    let mut killed = String::new();
    for _ in 0..10 {
        killed = tools.execute(&call("run_command", &poll)).await?;
        if killed.contains("state: dead") || killed.contains("state: exit") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        killed.contains("state: dead") || killed.contains("state: exit"),
        "{killed}"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}
