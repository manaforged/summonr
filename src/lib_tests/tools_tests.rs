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
async fn plan_commands_can_inspect_but_cannot_change_the_workspace() -> Result<()> {
    let workspace = test_workspace()?;
    let Some(_) = sandbox_for_test(&workspace)? else {
        std::fs::remove_dir_all(workspace)?;
        return Ok(());
    };
    let mut tools = crate::tools::ToolRegistry::new(&workspace)?;
    tools.enable_sandbox(true)?;
    tools.set_read_only(true);

    let read = tools
        .execute(&call(
            "run_command",
            r#"{"command":"cat note.txt >/dev/null && printf inspected"}"#,
        ))
        .await?;
    assert!(read.contains("inspected"), "{read}");

    let write = tools
        .execute(&call(
            "run_command",
            r#"{"command":"printf changed > note.txt"}"#,
        ))
        .await?;
    assert!(write.contains("exit 1"), "{write}");
    assert!(write.contains("Operation not permitted"), "{write}");
    assert_eq!(
        std::fs::read_to_string(workspace.join("note.txt"))?,
        "hello\nworld"
    );

    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn contained_commands_can_write_only_inside_the_workspace() -> Result<()> {
    let workspace = test_workspace()?;
    let Some(_) = sandbox_for_test(&workspace)? else {
        std::fs::remove_dir_all(workspace)?;
        return Ok(());
    };
    let mut tools = crate::tools::ToolRegistry::new(&workspace)?;
    tools.enable_sandbox(true)?;

    let written = tools
        .execute(&call(
            "run_command",
            r#"{"command":"printf changed > note.txt"}"#,
        ))
        .await?;
    assert!(written.starts_with("exit 0"), "{written}");
    assert_eq!(
        std::fs::read_to_string(workspace.join("note.txt"))?,
        "changed"
    );

    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn plan_mode_ignores_the_old_write_hint_and_stays_read_only() -> Result<()> {
    let workspace = test_workspace()?;
    let Some(_) = sandbox_for_test(&workspace)? else {
        std::fs::remove_dir_all(workspace)?;
        return Ok(());
    };
    let mut tools = crate::tools::ToolRegistry::new(&workspace)?;
    tools.enable_sandbox(true)?;
    tools.set_read_only(true);

    let output = tools
        .execute(&call(
            "run_command",
            r#"{"command":"printf changed > note.txt","write":true}"#,
        ))
        .await?;
    assert!(!output.starts_with("exit 0"), "{output}");
    assert_eq!(
        std::fs::read_to_string(workspace.join("note.txt"))?,
        "hello\nworld"
    );

    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn plan_commands_require_the_command_sandbox() -> Result<()> {
    let workspace = test_workspace()?;
    let mut tools = crate::tools::ToolRegistry::new(&workspace)?;
    tools.set_read_only(true);

    let error = tools
        .execute(&call("run_command", r#"{"command":"cat note.txt"}"#))
        .await
        .expect_err("Plan mode must not run an uncontained shell")
        .to_string();
    assert!(error.contains("requires the command sandbox"), "{error}");

    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn plan_commands_reject_background_work() -> Result<()> {
    let workspace = test_workspace()?;
    let Some(_) = sandbox_for_test(&workspace)? else {
        std::fs::remove_dir_all(workspace)?;
        return Ok(());
    };
    let mut tools = crate::tools::ToolRegistry::new(&workspace)?;
    tools.enable_sandbox(true)?;
    tools.set_read_only(true);

    let error = tools
        .execute(&call(
            "run_command",
            r#"{"command":"printf inspected","background":true}"#,
        ))
        .await
        .expect_err("Plan mode must not leave a background process")
        .to_string();
    assert!(error.contains("does not permit background"), "{error}");

    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn dropping_a_foreground_command_kills_its_process_group() -> Result<()> {
    let workspace = test_workspace()?;
    let pid_path = workspace.join("leader.pid");
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let task = tokio::spawn(async move {
        tools
            .execute(&call(
                "run_command",
                r#"{"command":"echo $$ > leader.pid; sleep 30 & wait"}"#,
            ))
            .await
    });

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while !pid_path.is_file() {
        if tokio::time::Instant::now() >= deadline {
            task.abort();
            return Err(Error::Tool("command did not publish its pid".to_string()));
        }
        tokio::task::yield_now().await;
    }
    let pid = std::fs::read_to_string(&pid_path)?.trim().to_string();
    task.abort();
    drop(task.await);

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::process::Command::new("kill")
        .args(["-0", &format!("-{pid}")])
        .status()
        .is_ok_and(|status| status.success())
    {
        if tokio::time::Instant::now() >= deadline {
            return Err(Error::Tool(format!(
                "process group {pid} survived command cancellation"
            )));
        }
        tokio::task::yield_now().await;
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
async fn edit_file_replaces_text_without_reproducing_the_line() -> Result<()> {
    let workspace = test_workspace()?;
    let path = workspace.join("prompt.rs");
    let original = "let prompt = \"Keep reasoning brief.\"; \\\nnext();\n";
    std::fs::write(&path, original)?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let line = original
        .lines()
        .next()
        .ok_or_else(|| Error::Tool("missing test line".into()))?;
    let args = serde_json::json!({
        "path": "prompt.rs",
        "replace_text": {
            "tag": line_hash(line),
            "old": "Keep reasoning brief.",
            "new": "Keep reasoning concise."
        }
    });

    tools
        .execute(&call("edit_file", &serde_json::to_string(&args)?))
        .await?;

    assert_eq!(
        std::fs::read_to_string(&path)?,
        "let prompt = \"Keep reasoning concise.\"; \\\nnext();\n"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn edit_file_replaces_unique_text_without_a_probe() -> Result<()> {
    let workspace = test_workspace()?;
    let path = workspace.join("prompt.rs");
    let original = "before\\\nKeep reasoning brief.\\\nafter\\\n";
    std::fs::write(&path, original)?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let args = serde_json::json!({
        "path": "prompt.rs",
        "replace_text": {
            "old": "Keep reasoning brief.",
            "new": "Keep reasoning concise."
        }
    });

    tools
        .execute(&call("edit_file", &serde_json::to_string(&args)?))
        .await?;

    assert_eq!(
        std::fs::read_to_string(&path)?,
        "before\\\nKeep reasoning concise.\\\nafter\\\n"
    );

    std::fs::write(&path, "brief\nbrief\n")?;
    let ambiguous = serde_json::json!({
        "path": "prompt.rs",
        "replace_text": {"old": "brief", "new": "concise"}
    });
    tools
        .execute(&call("edit_file", &serde_json::to_string(&ambiguous)?))
        .await
        .unwrap_err();
    assert_eq!(std::fs::read_to_string(&path)?, "brief\nbrief\n");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn edit_file_replaces_exact_multiline_text() -> Result<()> {
    let workspace = test_workspace()?;
    let path = workspace.join("prompt.rs");
    let original = "fn old() {\n    first();\n    second();\n}\n\nfn keep() {}\n";
    std::fs::write(&path, original)?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let args = serde_json::json!({
        "path": "prompt.rs",
        "edits": [{
            "tag": line_hash("fn old() {"),
            "old": "fn old() {\n    first();\n    second();\n}",
            "new": "fn new() {\n    done();\n}"
        }]
    });

    tools
        .execute(&call("edit_file", &serde_json::to_string(&args)?))
        .await?;

    assert_eq!(
        std::fs::read_to_string(&path)?,
        "fn new() {\n    done();\n}\n\nfn keep() {}\n"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn edit_file_replace_text_rejects_unsafe_matches_before_writing() -> Result<()> {
    let workspace = test_workspace()?;
    let path = workspace.join("repeat.txt");
    let original = "brief brief\n";
    std::fs::write(&path, original)?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let tag = line_hash("brief brief");

    for (old, new) in [("brief", "short"), ("missing", "short")] {
        let args = serde_json::json!({
            "path": "repeat.txt",
            "replace_text": {"tag": tag, "old": old, "new": new}
        });
        tools
            .execute(&call("edit_file", &serde_json::to_string(&args)?))
            .await
            .unwrap_err();
        assert_eq!(std::fs::read_to_string(&path)?, original);
    }
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn read_file_rejects_paths_outside_workspace() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let relative = tools
        .execute(&call("read_file", r#"{"path":"../outside.txt"}"#))
        .await;
    relative.unwrap_err();
    let args = serde_json::json!({"path": workspace.canonicalize()?.parent()});
    let absolute = tools
        .execute(&call("read_file", &serde_json::to_string(&args)?))
        .await;
    absolute.unwrap_err();
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn read_file_accepts_an_absolute_workspace_path() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let args = serde_json::json!({"path": workspace.canonicalize()?.join("note.txt")});
    let result = tools
        .execute(&call("read_file", &serde_json::to_string(&args)?))
        .await?;
    assert!(result.contains("hello"), "{result}");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn unrestricted_tools_read_and_write_outside_the_workspace() -> Result<()> {
    let workspace = test_workspace()?;
    let outside = outside_fixture(&workspace, "unrestricted-tools");
    let mut tools = crate::tools::ToolRegistry::new(&workspace)?;
    tools.set_unrestricted();
    let write = serde_json::json!({"path": &outside, "content": "outside\n"});
    tools
        .execute(&call("write_file", &serde_json::to_string(&write)?))
        .await?;
    let read = serde_json::json!({"path": &outside});
    let result = tools
        .execute(&call("read_file", &serde_json::to_string(&read)?))
        .await?;
    assert!(result.contains("outside"), "{result}");
    std::fs::remove_file(outside)?;
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn read_file_keeps_normal_window_output() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::write(workspace.join("small.txt"), "one\ntwo\nthree\n")?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let out = tools
        .execute(&call(
            "read_file",
            r#"{"path":"small.txt","offset":2,"limit":1}"#,
        ))
        .await?;
    assert!(out.starts_with("small.txt  lines 2-2/3"), "{out}");
    assert!(
        out.contains(&format!("2:{}|two", line_hash("two"))),
        "{out}"
    );
    assert!(!out.contains("unknown"), "{out}");
    std::fs::write(workspace.join("small.txt"), "one\ntwo")?;
    let out = tools
        .execute(&call("read_file", r#"{"path":"small.txt","offset":2}"#))
        .await?;
    assert_eq!(
        out,
        format!("small.txt  lines 2-2/2\n2:{}|two", line_hash("two"))
    );
    std::fs::write(workspace.join("small.txt"), b"unfinished \xc3")?;
    let error = tools
        .execute(&call("read_file", r#"{"path":"small.txt"}"#))
        .await
        .expect_err("invalid UTF-8 at EOF must not be treated as a read limit");
    assert!(error.to_string().contains("not valid UTF-8"));
    std::fs::write(workspace.join("empty.txt"), "")?;
    let empty = tools
        .execute(&call("read_file", r#"{"path":"empty.txt"}"#))
        .await?;
    assert_eq!(empty, "empty.txt  lines 0-0/0");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn read_file_never_renumbers_lines_after_a_clipped_window() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::write(
        workspace.join("window.txt"),
        format!("first\n{}\ntail\n", "x".repeat(2 * 1024 * 1024)),
    )?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let out = tools
        .execute(&call("read_file", r#"{"path":"window.txt"}"#))
        .await?;
    assert!(
        out.contains("lines 1-1/3"),
        "{}",
        &out[..out.len().min(100)]
    );
    assert!(!out.contains("|tail"));
    assert!(out.contains("continue with offset 2"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn read_file_bounds_large_prefix_and_reports_unknown_total() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::write(
        workspace.join("large.txt"),
        format!("first\n{}", "x".repeat(9 * 1024 * 1024)),
    )?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let head = tools
        .execute(&call(
            "read_file",
            r#"{"path":"large.txt","offset":1,"limit":1}"#,
        ))
        .await?;
    assert!(head.contains("lines 1-1/unknown"), "{head}");
    assert!(head.contains("total line count unknown"), "{head}");
    assert!(
        head.len() < 1000,
        "the selected short line is bounded: {}",
        head.len()
    );

    let giant = tools
        .execute(&call(
            "read_file",
            r#"{"path":"large.txt","offset":2,"limit":1}"#,
        ))
        .await?;
    assert!(giant.contains("line clipped"), "{giant}");
    assert!(giant.contains("lines 2-2/unknown"), "{giant}");
    assert!(giant.len() < 2_200_000, "{} bytes", giant.len());
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn write_file_preserves_mode_and_ignores_prepared_temp_symlink() -> Result<()> {
    let workspace = test_workspace()?;
    let target = workspace.join("script.sh");
    std::fs::write(&target, "old\n")?;
    let mut permissions = std::fs::metadata(&target)?.permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o751);
    std::fs::set_permissions(&target, permissions)?;

    let outside = tempfile::tempdir()?;
    let sentinel = outside.path().join("sentinel.txt");
    std::fs::write(&sentinel, "sentinel\n")?;
    std::os::unix::fs::symlink(&sentinel, workspace.join("script.sh.mage-tmp"))?;

    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    tools
        .execute(&call(
            "write_file",
            r#"{"path":"script.sh","content":"new\n"}"#,
        ))
        .await?;

    assert_eq!(std::fs::read_to_string(&target)?, "new\n");
    let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&target)?.permissions());
    assert_eq!(mode & 0o777, 0o751);
    assert_eq!(std::fs::read_to_string(&sentinel)?, "sentinel\n");
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
async fn run_command_uses_the_last_pipeline_status() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let stopped = tools
        .execute_outcome(&call(
            "run_command",
            r#"{"command":"yes | head -n 1 >/dev/null && echo after"}"#,
        ))
        .await?;
    assert!(stopped.success);
    assert!(stopped.text.contains("after"));
    let failed = tools
        .execute_outcome(&call("run_command", r#"{"command":"true | false"}"#))
        .await?;
    assert!(!failed.success);
    assert!(failed.text.contains("exit 1"));
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
async fn repeated_command_runs_again() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let first = tools
        .execute(&call(
            "run_command",
            r#"{"command":"printf x >> count.txt; wc -c < count.txt"}"#,
        ))
        .await?;
    let second = tools
        .execute(&call(
            "run_command",
            r#"{"command":"printf x >> count.txt; wc -c < count.txt"}"#,
        ))
        .await?;
    assert!(first.ends_with('1'), "{first}");
    assert!(second.ends_with('2'), "{second}");
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

#[test]
fn editing_tools_advertise_flat_operation_shapes() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let definitions = tools.definitions();
    assert_eq!(
        definitions
            .iter()
            .filter(|tool| matches!(
                tool.function.name.as_str(),
                "replace_text"
                    | "replace_texts"
                    | "replace_lines"
                    | "insert_after"
                    | "delete_lines"
            ))
            .count(),
        5
    );
    assert!(
        definitions
            .iter()
            .all(|tool| tool.function.name != "edit_file")
    );
    let replace = definitions
        .iter()
        .find(|tool| tool.function.name == "replace_lines")
        .expect("replace_lines definition");
    assert_eq!(
        replace.function.parameters["required"],
        serde_json::json!(["path", "tag", "new"])
    );
    assert_eq!(
        replace.function.parameters["properties"]["tag"]["type"],
        "string"
    );
    let batch = definitions
        .iter()
        .find(|tool| tool.function.name == "replace_texts")
        .expect("replace_texts definition");
    assert!(
        batch.function.parameters["properties"]["edits"]
            .get("maxItems")
            .is_none()
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn flat_editing_tools_apply_each_operation() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(workspace.join("f.txt"), "alpha\nbeta\ngamma\n")?;
    tools
        .execute(&call(
            "replace_text",
            r#"{"path":"f.txt","old":"ph","new":"PH"}"#,
        ))
        .await?;
    let beta = crate::hashline::line_hash("beta");
    tools
        .execute(&call(
            "replace_lines",
            &format!(r#"{{"path":"f.txt","tag":"{beta}","new":"BETA"}}"#),
        ))
        .await?;
    let beta = crate::hashline::line_hash("BETA");
    tools
        .execute(&call(
            "insert_after",
            &format!(r#"{{"path":"f.txt","tag":"{beta}","text":"between"}}"#),
        ))
        .await?;
    let gamma = crate::hashline::line_hash("gamma");
    tools
        .execute(&call(
            "delete_lines",
            &format!(r#"{{"path":"f.txt","tag":"{gamma}"}}"#),
        ))
        .await?;
    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "alPHa\nBETA\nbetween\n"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn replace_texts_applies_atomic_flat_edits() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(workspace.join("f.txt"), "alpha\nbeta\ngamma\n")?;

    tools
        .execute(&call(
            "replace_texts",
            r#"{"path":"f.txt","edits":[{"old":"alpha","new":"ALPHA"},{"old":"gamma","new":"GAMMA"}]}"#,
        ))
        .await?;

    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "ALPHA\nbeta\nGAMMA\n"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn replace_texts_applies_dependent_edits_in_order() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(workspace.join("f.txt"), "alpha beta\n")?;

    tools
        .execute(&call(
            "replace_texts",
            r#"{"path":"f.txt","edits":[{"old":"alpha","new":"ALPHA"},{"old":"ALPHA beta","new":"done"}]}"#,
        ))
        .await?;

    assert_eq!(std::fs::read_to_string(workspace.join("f.txt"))?, "done\n");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn replace_texts_relocates_original_anchor_after_line_shift() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(
        workspace.join("f.txt"),
        "head\none\ntarget alpha\ntarget beta\n",
    )?;
    let target = crate::hashline::line_hash("target alpha");

    tools
        .execute(&call(
            "replace_texts",
            &format!(
                r#"{{"path":"f.txt","edits":[{{"old":"head","new":"head\ninserted"}},{{"tag":"3:{target}","old":"target","new":"TARGET"}}]}}"#
            ),
        ))
        .await?;

    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "head\ninserted\none\nTARGET alpha\ntarget beta\n"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn replace_text_relocates_unchanged_line_after_prior_edit() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(workspace.join("f.txt"), "head\ntarget alpha\ntarget beta\n")?;
    let target = crate::hashline::line_hash("target alpha");
    tools
        .execute(&call(
            "replace_text",
            r#"{"path":"f.txt","old":"head","new":"head\ninserted"}"#,
        ))
        .await?;

    tools
        .execute(&call(
            "replace_text",
            &format!(r#"{{"path":"f.txt","tag":"2:{target}","old":"target","new":"TARGET"}}"#),
        ))
        .await?;

    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "head\ninserted\nTARGET alpha\ntarget beta\n"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn replace_text_does_not_relocate_an_ambiguous_hash() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(workspace.join("f.txt"), "head\ntarget\ntarget\n")?;
    let target = crate::hashline::line_hash("target");
    tools
        .execute(&call(
            "replace_text",
            r#"{"path":"f.txt","old":"head","new":"head\ninserted"}"#,
        ))
        .await?;

    let error = tools
        .execute(&call(
            "replace_text",
            &format!(r#"{{"path":"f.txt","tag":"2:{target}","old":"target","new":"TARGET"}}"#),
        ))
        .await
        .expect_err("a colliding hash must not choose a target");

    assert!(error.to_string().contains("stale tag"));
    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "head\ninserted\ntarget\ntarget\n"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn replace_text_uses_globally_unique_old_despite_nearby_tag() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(
        workspace.join("f.txt"),
        "match entry {\n    Entry::User => user(),\n    Entry::Aborted => stop(),\n}\n",
    )?;
    let container = crate::hashline::line_hash("match entry {");

    tools
        .execute(&call(
            "replace_text",
            &format!(
                r#"{{"path":"f.txt","tag":"1:{container}","old":"    Entry::Aborted => stop(),","new":"    Entry::Failed => fail(),\n    Entry::Aborted => stop(),"}}"#
            ),
        ))
        .await?;

    assert!(std::fs::read_to_string(workspace.join("f.txt"))?.contains("Entry::Failed"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn failed_replace_texts_returns_fresh_anchor_context() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let original = "alpha\nbeta\ngamma\n";
    std::fs::write(workspace.join("f.txt"), original)?;
    let beta = crate::hashline::line_hash("beta");

    let error = tools
        .execute(&call(
            "replace_texts",
            &format!(
                r#"{{"path":"f.txt","edits":[{{"old":"alpha","new":"ALPHA"}},{{"tag":"{beta}","old":"missing","new":"GAMMA"}}]}}"#
            ),
        ))
        .await
        .expect_err("the stale second edit must reject the batch");

    let message = error.to_string();
    assert!(message.contains("edit 2:"), "{message}");
    assert!(message.contains("Current lines:"), "{message}");
    assert!(message.contains(&format!("2:{beta}|beta")), "{message}");
    assert_eq!(std::fs::read_to_string(workspace.join("f.txt"))?, original);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn edit_file_explains_a_malformed_nested_replace() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(workspace.join("f.txt"), "before\nafter\n")?;
    let before = crate::hashline::line_hash("before");
    let after = crate::hashline::line_hash("after");
    let args = format!(
        r#"{{"path":"f.txt","edits":[{{"tag":"{before}","end_tag":"{after}","replace":"new"}}]}}"#
    );
    let error = tools
        .execute(&call("edit_file", &args))
        .await
        .expect_err("a string is not a replace operation")
        .to_string();
    assert!(error.contains("replace must be an object"), "{error}");
    assert!(error.contains("use replace_text"), "{error}");
    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "before\nafter\n"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn edit_tags_reject_ambiguous_ranges_and_explain_misses() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(
        workspace.join("f.txt"),
        "fn a() {\n    1\n}\nfn b() {\n    2\n}\n",
    )?;
    let start = crate::hashline::line_hash("fn a() {");
    let close = crate::hashline::line_hash("}");
    let ambiguous = format!(
        r#"{{"path":"f.txt","replace":{{"tag":"{start}","end_tag":"{close}","new":"fn a() {{ 0 }}"}}}}"#
    );
    let error = tools
        .execute(&call("edit_file", &ambiguous))
        .await
        .expect_err("a repeated range end must be numbered")
        .to_string();
    assert!(error.contains("matches 2 lines"), "{error}");
    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "fn a() {\n    1\n}\nfn b() {\n    2\n}\n"
    );
    let args = format!(
        r#"{{"path":"f.txt","replace":{{"tag":"{start}","end_tag":"3:{close}","new":"fn a() {{ 0 }}"}}}}"#
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
async fn failed_flat_edit_returns_fresh_anchor_context() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    std::fs::write(workspace.join("f.txt"), "alpha\nbeta\ngamma\n")?;
    let beta = crate::hashline::line_hash("beta");
    let error = tools
        .execute(&call(
            "replace_text",
            &format!(r#"{{"path":"f.txt","tag":"{beta}","old":"missing","new":"BETA"}}"#),
        ))
        .await
        .map(|_| String::new())
        .unwrap_or_else(|error| error.to_string());
    assert!(error.contains("Current lines:"), "{error}");
    assert!(error.contains(&format!("2:{beta}|beta")), "{error}");
    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "alpha\nbeta\ngamma\n"
    );
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

    let one = crate::hashline::line_hash("ONE");
    tools
        .execute(&call(
            "edit_file",
            &format!(
                r#"{{"path":"f.txt","edits":[{{"replace_text":"{one}","old":"ON","new":"on"}}]}}"#
            ),
        ))
        .await?;
    assert_eq!(
        std::fs::read_to_string(workspace.join("f.txt"))?,
        "onE\nbetween\n"
    );

    let one = crate::hashline::line_hash("onE");
    tools
        .execute(&call(
            "edit_file",
            &format!(
                r#"{{"path":"f.txt","edits":[{{"type":"replace_text","tag":"{one}","old":"on","new":"ON"}}]}}"#
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
                    _ => {
                        assert!(request.messages.last().is_some_and(|message| {
                            message.role == crate::provider::Role::Tool
                                && message.content.starts_with("exit 3")
                        }));
                        Ok(AssistantMessage {
                            content: Some("handled".to_string()),
                            ..AssistantMessage::default()
                        })
                    }
                }
            })
        }
    }

    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        ExitThenText(AtomicUsize::new(0)),
        "test-model",
        &workspace,
        "test",
    )?;
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
        .execute_outcome(&call(
            "run_command",
            r#"{"command":"sleep 5","timeout_secs":1}"#,
        ))
        .await?;
    assert!(!result.success);
    assert!(result.mutated);
    assert!(result.text.contains("command timed out after 1s"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn run_command_streams_large_output_without_workspace_state() -> Result<()> {
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
    assert!(!workspace.join(".mage").exists());
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
        if killed.contains("state: dead")
            || killed.contains("state: exit")
            || killed.contains("state: killed")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        killed.contains("state: dead")
            || killed.contains("state: exit")
            || killed.contains("state: killed"),
        "{killed}"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn replace_text_handles_multibyte_old() -> Result<()> {
    let workspace = test_workspace()?;
    let path = workspace.join("m.rs");
    std::fs::write(&path, "label: café\nrest\n")?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let args = serde_json::json!({
        "path": "m.rs",
        "replace_text": {"old": "café", "new": "X"}
    });
    tools
        .execute(&call("edit_file", &serde_json::to_string(&args)?))
        .await?;
    assert_eq!(std::fs::read_to_string(&path)?, "label: X\nrest\n");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn replace_text_with_old_ending_in_newline_merges() -> Result<()> {
    let workspace = test_workspace()?;
    let path = workspace.join("j.rs");
    std::fs::write(&path, "join me\nnext\n")?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let args = serde_json::json!({
        "path": "j.rs",
        "replace_text": {"old": "me\n", "new": "ME"}
    });
    tools
        .execute(&call("edit_file", &serde_json::to_string(&args)?))
        .await?;
    assert_eq!(std::fs::read_to_string(&path)?, "join MEnext\n");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn replace_text_preserves_crlf_endings() -> Result<()> {
    let workspace = test_workspace()?;
    let path = workspace.join("crlf.rs");
    std::fs::write(&path, "a = 1\r\nb = 2\r\nc = 3\r\n")?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let args = serde_json::json!({
        "path": "crlf.rs",
        "replace_text": {"old": "b = 2", "new": "b = 9"}
    });
    tools
        .execute(&call("edit_file", &serde_json::to_string(&args)?))
        .await?;
    assert_eq!(
        std::fs::read_to_string(&path)?,
        "a = 1\r\nb = 9\r\nc = 3\r\n"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn replace_text_counts_overlap_as_one_placement() -> Result<()> {
    let workspace = test_workspace()?;
    let path = workspace.join("o.rs");
    std::fs::write(&path, "aaa\n")?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let args = serde_json::json!({
        "path": "o.rs",
        "replace_text": {"old": "aa", "new": "X"}
    });
    tools
        .execute(&call("edit_file", &serde_json::to_string(&args)?))
        .await?;
    assert_eq!(std::fs::read_to_string(&path)?, "Xa\n");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn replace_texts_has_no_fixed_batch_limit() -> Result<()> {
    let workspace = test_workspace()?;
    let path = workspace.join("big.txt");
    let original = (0..17)
        .map(|i| format!("line-{i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, &original)?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let edits: Vec<_> = (0..17)
        .map(|i| serde_json::json!({"old": format!("line-{i:02}"), "new": format!("done-{i:02}")}))
        .collect();
    let args = serde_json::json!({"path": "big.txt", "edits": edits});
    tools
        .execute(&call("replace_texts", &serde_json::to_string(&args)?))
        .await?;
    let expected = (0..17)
        .map(|i| format!("done-{i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(std::fs::read_to_string(&path)?, expected);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn grep_skips_gitignored_trees() -> Result<()> {
    let workspace = test_workspace()?;
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "s@t"],
        vec!["config", "user.name", "S"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&workspace)
                .args(args)
                .status()?
                .success()
        );
    }
    std::fs::write(workspace.join(".gitignore"), ".venv/\n")?;
    std::fs::create_dir_all(workspace.join(".venv/lib"))?;
    std::fs::write(workspace.join(".venv/lib/x.py"), "needle in venv\n")?;
    std::fs::create_dir_all(workspace.join("src"))?;
    std::fs::write(workspace.join("src/a.rs"), "needle in source\n")?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let out = tools
        .execute(&call("grep", r#"{"pattern":"needle"}"#))
        .await?;
    assert!(out.starts_with("1 matches"), "{out}");
    assert!(out.contains("src/a.rs:1:"), "{out}");
    assert!(!out.contains(".venv"), "{out}");
    let scoped = tools
        .execute(&call("grep", r#"{"pattern":"needle","path":"src"}"#))
        .await?;
    assert!(scoped.contains("src/a.rs:1:"), "{scoped}");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn read_file_windows_a_file_larger_than_the_byte_cap() -> Result<()> {
    let workspace = test_workspace()?;
    let line = "x".repeat(3000);
    let big: String = (0..3000).map(|i| format!("{i} {line}\n")).collect();
    std::fs::write(workspace.join("big.txt"), &big)?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let out = tools
        .execute(&call(
            "read_file",
            r#"{"path":"big.txt","offset":5,"limit":3}"#,
        ))
        .await?;
    assert!(out.contains("|4 "), "{out}");
    assert!(out.contains("|6 "), "{out}");
    assert!(!out.contains("|7 "), "{out}");
    let wide = tools
        .execute(&call("read_file", r#"{"path":"big.txt","limit":1000}"#))
        .await?;
    assert!(
        wide.contains("[truncated at"),
        "a 3 MB window is cut with a notice"
    );
    assert!(
        wide.contains("continue with offset"),
        "{}",
        &wide[wide.len() - 120..]
    );
    assert!(
        !crate::tools::readonly("definition"),
        "definition can rename"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn grep_skips_symlinks_and_searches_an_ignored_path_on_request() -> Result<()> {
    let workspace = test_workspace()?;
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "s@t"],
        vec!["config", "user.name", "S"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&workspace)
                .args(args)
                .status()?
                .success()
        );
    }
    let outside = tempfile::tempdir()?;
    std::fs::write(outside.path().join("secret.txt"), "needle outside\n")?;
    std::os::unix::fs::symlink(outside.path().join("secret.txt"), workspace.join("leak"))?;
    std::fs::write(workspace.join(".gitignore"), "vendor/\n")?;
    std::fs::create_dir_all(workspace.join("vendor"))?;
    std::fs::write(workspace.join("vendor/lib.py"), "needle vendored\n")?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let out = tools
        .execute(&call("grep", r#"{"pattern":"needle"}"#))
        .await?;
    assert!(!out.contains("leak"), "{out}");
    assert!(!out.contains("vendor"), "{out}");
    let scoped = tools
        .execute(&call("grep", r#"{"pattern":"needle","path":"vendor"}"#))
        .await?;
    assert!(scoped.contains("vendor/lib.py:1:"), "{scoped}");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn an_empty_file_says_use_write_file() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::write(workspace.join("empty.txt"), "")?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let error = tools
        .execute(&call(
            "insert_after",
            r#"{"path":"empty.txt","tag":"1:0000","text":"x"}"#,
        ))
        .await
        .expect_err("nothing to anchor")
        .to_string();
    assert!(error.contains("use write_file"), "{error}");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn one_giant_line_is_clipped_not_returned_whole() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::write(
        workspace.join("wide.txt"),
        format!("{}\nnext\n", "y".repeat(3 * 1024 * 1024)),
    )?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let out = tools
        .execute(&call("read_file", r#"{"path":"wide.txt"}"#))
        .await?;
    assert!(out.len() < 2_200_000, "{} bytes", out.len());
    assert!(
        out.contains("[line clipped at"),
        "{}",
        &out[out.len() - 200..]
    );
    assert!(
        out.contains("continue with offset 2"),
        "the next line is on the next window"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn a_writing_command_counts_as_a_mutation_and_a_reading_one_does_not() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let read = tools
        .execute_outcome(&call("run_command", r#"{"command":"cat note.txt"}"#))
        .await?;
    assert!(!read.mutated);
    let write = tools
        .execute_outcome(&call("run_command", r#"{"command":"touch made.txt"}"#))
        .await?;
    assert!(write.mutated);
    let git = tools
        .execute_outcome(&call("run_command", r#"{"command":"git --version"}"#))
        .await?;
    assert!(git.mutated, "an unknown git verb is not read-only");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn read_file_streams_a_window_past_bad_bytes_and_keeps_line_endings() -> Result<()> {
    let workspace = test_workspace()?;
    let mut bytes = b"one\ntwo\n".to_vec();
    bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
    bytes.extend_from_slice(b"four\n");
    std::fs::write(workspace.join("mixed.bin"), &bytes)?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let head = tools
        .execute(&call("read_file", r#"{"path":"mixed.bin","limit":2}"#))
        .await?;
    assert!(head.contains("lines 1-2/4"), "{head}");
    let error = tools
        .execute(&call("read_file", r#"{"path":"mixed.bin"}"#))
        .await
        .expect_err("bad bytes in the window")
        .to_string();
    assert!(error.contains("line 3"), "{error}");

    std::fs::write(workspace.join("endings.txt"), "one\ntwo\r\nthree\n")?;
    let out = tools
        .execute(&call(
            "replace_text",
            r#"{"path":"endings.txt","old":"one","new":"ONE"}"#,
        ))
        .await?;
    assert!(out.starts_with("edited"), "{out}");
    assert_eq!(
        std::fs::read(workspace.join("endings.txt"))?,
        b"ONE\ntwo\r\nthree\n",
        "each untouched line keeps its own terminator"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}
