use super::*;

#[test]
fn a_background_deadline_is_rejected_before_the_command_runs() {
    let (dir, tools) = workspace(&[]);
    let refused = outcome_with(
        &tools,
        serde_json::json!({
            "command": "echo launched > deadline.txt",
            "background": true,
            "timeout_secs": 1,
        }),
    );
    assert!(
        refused.contains("timeout_secs applies to foreground"),
        "{refused}"
    );
    assert!(!dir.path().join("deadline.txt").exists(), "{refused}");
}

#[test]
fn output_under_the_budget_comes_back_whole_with_no_log_note() {
    let (_dir, tools) = workspace(&[]);
    let text = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "seq 1 300"}),
    );
    assert!(text.contains("\n150\n") && text.contains("300"), "{text}");
    assert!(!text.contains("read the full output"), "{text}");
}

#[test]
fn ls_runs_in_the_shell_instead_of_turning_into_a_read() {
    let (_dir, tools) = workspace(&[("a.txt", "x\n")]);
    let text = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "ls a.txt"}),
    );
    assert!(text.contains("a.txt"), "{text}");
    assert!(!text.contains("answers this natively"), "{text}");
}

#[test]
fn a_command_past_the_wait_limit_keeps_running_as_a_job() {
    let (_dir, mut tools) = workspace(&[]);
    let user = [
        (crate::modules::ModuleId::Search, false),
        (crate::modules::ModuleId::Lsp, false),
        (crate::modules::ModuleId::Jobs, true),
        (crate::modules::ModuleId::Skills, false),
    ]
    .into();
    tools
        .set_modules(&crate::modules::Switches::from_maps(
            &user,
            &std::collections::BTreeMap::new(),
        ))
        .expect("module setup");
    tools.set_session("s1");
    let started = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "sleep 2; echo finished"}),
    );
    let id = started
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .unwrap_or_else(|| panic!("no job id in {started}"))
        .to_string();
    assert!(
        started.contains(&format!("run_command {{\"job\":\"{id}\"}}")),
        "{started}"
    );
    let polled = run(&tools, "run_command", serde_json::json!({"job": id}));
    assert!(polled.contains("finished"), "{polled}");
}

#[test]
fn a_long_output_line_reaches_the_model_whole() {
    let (_dir, tools) = workspace(&[]);
    let text = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "printf 'x%.0s' $(seq 1 2000); echo"}),
    );
    assert!(!text.contains("clipped"), "{text}");
    assert!(!text.contains("full output"), "{text}");
    assert!(text.contains(&"x".repeat(2000)), "{} bytes", text.len());
}

#[test]
fn a_cat_with_a_workdir_reads_from_that_directory() {
    let (dir, tools) = workspace(&[("note.txt", "top\n")]);
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/note.txt"), "inner\n").unwrap();
    let text = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "cat note.txt", "workdir": "sub"}),
    );
    assert!(text.contains("inner"), "{text}");
}

#[test]
fn a_command_that_cannot_become_a_job_outlives_the_job_wait() {
    let (_dir, mut tools) = workspace(&[]);
    let user = [(crate::modules::ModuleId::Jobs, false)].into();
    tools
        .set_modules(&crate::modules::Switches::from_maps(
            &user,
            &std::collections::BTreeMap::new(),
        ))
        .expect("module setup");
    tools.set_session("s1");
    let output = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "sleep 2; echo finished"}),
    );
    assert!(output.starts_with("exit 0"), "{output}");
    assert!(output.contains("finished"), "{output}");
}

#[test]
fn the_full_output_note_counts_every_line_of_a_large_log() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("out.log");
    let line = "x".repeat(1023) + "\n";
    std::fs::write(&log, line.repeat(700)).unwrap();
    let note = spill::full_output_note(&log, true, crate::diff::line_count(&log).ok().flatten());
    assert!(note.ends_with("(700 lines)]"), "got {note}");
    assert_eq!(spill::full_output_note(&log, false, None), "");
}

#[cfg(sandboxed)]
#[test]
fn a_contained_read_only_registry_runs_read_commands_and_refuses_writes() {
    let (dir, mut tools) = workspace(&[("notes.txt", "one\ntwo\n")]);
    tools.contain_read_only();
    let counted = run(
        &tools,
        "run_command",
        serde_json::json!({"command": "wc -l notes.txt"}),
    );
    assert!(counted.contains("2 notes.txt"), "{counted}");
    let call = ToolCall {
        id: "t2".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command": "echo x > made.txt"}).to_string(),
        },
        continuation: None,
    };
    drop(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(tools.execute_outcome(&call)),
    );
    assert!(!dir.path().join("made.txt").exists());
}

#[test]
fn only_catastrophic_commands_ask_and_strict_refuses_every_hazard() {
    let (_dir, mut tools) = workspace(&[]);
    let wipe = "dd if=/dev/zero of=/dev/null count=1";
    for command in [wipe.to_string(), format!("bash <<'EOF'\n{wipe}\nEOF")] {
        let asked = outcome(&tools, &command);
        assert!(asked.contains("needs the user's approval"), "{asked}");
    }
    let written = outcome(&tools, &format!("cat > notes.md <<'EOF'\n{wipe}\nEOF"));
    assert!(!written.contains("needs the user's approval"), "{written}");
    let routine = outcome(&tools, "git stash drop --help >/dev/null 2>&1; echo ran");
    assert!(routine.contains("ran"), "{routine}");
    tools.set_strict();
    let refused = outcome(&tools, "sudo true");
    assert!(refused.contains("strict mode never runs it"), "{refused}");
}

#[test]
fn a_command_in_another_workdir_is_judged_from_that_directory() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("ws");
    std::fs::create_dir_all(workspace.join("keep")).unwrap();
    std::fs::create_dir_all(root.path().join("keep")).unwrap();
    let tools = ToolRegistry::with_policy_and_search(&workspace, Policy::builtin(), None).unwrap();
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "rm -rf keep",
                "workdir": root.path().display().to_string(),
            })
            .to_string(),
        },
        continuation: None,
    };
    let result = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call));
    let text = match result {
        Ok(output) => output.text,
        Err(error) => error.to_string(),
    };
    assert!(root.path().join("keep").exists(), "{text}");
}

#[test]
fn a_timed_out_command_is_a_change_only_when_it_can_write() {
    let (dir, tools) = workspace(&[]);
    let timed_out = |command: &str| {
        let call = ToolCall {
            id: "t1".to_string(),
            kind: ToolKind::Function,
            function: ToolFunction {
                checked: Default::default(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command": command, "timeout_secs": 1}).to_string(),
            },
            continuation: None,
        };
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
            .block_on(tools.execute_outcome(&call))
            .unwrap()
    };
    let search = timed_out("mdfind -name zzq 2>/dev/null; locate zzq 2>&1; tail -f /dev/null");
    assert!(search.text.contains("timed out"), "{}", search.text);
    assert!(!search.mutated, "{}", search.text);
    assert!(!search.text.contains("may have changed"), "{}", search.text);
    for (command, file) in [
        ("tail -f /dev/null > out.txt", "out.txt"),
        ("tail -f /dev/null > 1", "1"),
        ("tail -f /dev/null 3> fd.txt", "fd.txt"),
    ] {
        let redirected = timed_out(command);
        assert!(redirected.mutated, "{command}: {}", redirected.text);
        assert!(
            redirected.text.contains("may have changed"),
            "{command}: {}",
            redirected.text
        );
        assert!(dir.path().join(file).exists(), "{command}");
    }
}

#[test]
fn a_failed_command_that_wrote_a_file_is_a_change() {
    let (dir, tools) = workspace(&[]);
    let call = ToolCall {
        id: "f1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command": "printf changed > changed.txt; exit 1"})
                .to_string(),
        },
        continuation: None,
    };
    let output = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call))
        .unwrap();
    assert!(!output.success && output.mutated, "{}", output.text);
    assert!(dir.path().join("changed.txt").exists());
}

#[test]
fn a_protected_read_is_refused_from_any_working_directory() {
    let (_dir, tools) = workspace(&[]);
    for (command, workdir) in [
        ("wc -l .ssh/config", Some("~")),
        ("cd ~ && wc -l .ssh/config", None),
        ("cd \"$HOME\" && wc -l .ssh/config", None),
        ("cd && wc -l .ssh/config", None),
        ("cp ~/.ssh/config copied.txt", None),
    ] {
        let mut arguments = serde_json::json!({ "command": command });
        if let Some(workdir) = workdir {
            arguments["workdir"] = serde_json::Value::from(workdir);
        }
        let refused = outcome_with(&tools, arguments);
        assert!(refused.contains("protected path"), "{command}: {refused}");
    }
}

#[test]
fn a_content_search_over_a_folder_holding_a_protected_path_is_refused() {
    let (_dir, tools) = workspace(&[("notes.txt", "needle\n")]);
    for (command, workdir) in [
        ("grep -r CANARY ~", None),
        ("grep -r CANARY \"$HOME\"", None),
        ("rg CANARY ~/", None),
        ("ag CANARY ~", None),
        ("grep -r CANARY /", None),
        ("grep -r CANARY .", Some("~")),
        ("cd && grep -r CANARY .", None),
        ("cp -r ~ copied", None),
    ] {
        let mut arguments = serde_json::json!({ "command": command });
        if let Some(workdir) = workdir {
            arguments["workdir"] = serde_json::Value::from(workdir);
        }
        let refused = outcome_with(&tools, arguments);
        assert!(refused.contains("protected path"), "{command}: {refused}");
    }
    for command in ["find ~ -maxdepth 0 -name x", "grep -r needle ."] {
        let ran = outcome_with(&tools, serde_json::json!({ "command": command }));
        assert!(!ran.contains("protected path"), "{command}: {ran}");
    }
}

#[test]
fn approval_toggle_gates_execution_and_preserves_one_shot_command_approval() {
    use super::super::command::Approval;

    let (dir, mut tools) = workspace(&[]);
    let command = "git clean -fdn; printf approved > ran.txt";
    assert!(tools.toggle_approval());
    let asked = outcome(&tools, command);
    assert!(asked.contains("needs the user's approval"), "{asked}");
    assert!(!dir.path().join("ran.txt").exists());
    tools.approve_once(Approval::Policy).expect("approve once");
    let approved = outcome(&tools, command);
    assert!(approved.starts_with("exit 0"), "{approved}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("ran.txt")).expect("command marker"),
        "approved"
    );
    std::fs::remove_file(dir.path().join("ran.txt")).expect("reset marker");
    let again = outcome(&tools, command);
    assert!(again.contains("needs the user's approval"), "{again}");
    assert!(!dir.path().join("ran.txt").exists());
    assert!(!tools.toggle_approval());
    let automatic = outcome(&tools, command);
    assert!(automatic.starts_with("exit 0"), "{automatic}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("ran.txt")).expect("auto marker"),
        "approved"
    );
}
