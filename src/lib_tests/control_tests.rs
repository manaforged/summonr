use super::*;

#[tokio::test]
async fn max_rounds_keeps_history() -> Result<()> {
    let workspace = test_workspace()?;
    struct RepeatRead;
    impl Provider for RepeatRead {
        fn complete<'a>(
            &'a self,
            _request: &'a crate::provider::CompletionRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<AssistantMessage>> + Send + 'a>,
        > {
            Box::pin(async {
                Ok(AssistantMessage {
                    content: None,
                    tool_calls: vec![ToolCall {
                        id: "c".to_string(),
                        kind: ToolKind::Function,
                        function: ToolFunction {
                            name: "read_file".to_string(),
                            arguments: r#"{"path":"note.txt"}"#.to_string(),
                        },
                    }],
                    ..AssistantMessage::default()
                })
            })
        }
    }
    let mut loop_ =
        Loop::with_session(RepeatRead, "test-model", &workspace, "test")?.with_max_rounds(2);
    let turn = loop_.turn("again").await?;
    assert_eq!(turn.rounds, 2);
    assert!(turn.text.contains("stopped after 2"));
    assert!(loop_.history_len() > 0);
    assert!(turn.aborted);
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert!(log.contains("read_file"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn default_rounds_continue_past_the_old_limit() -> Result<()> {
    struct RepeatRead(AtomicUsize);
    impl Provider for RepeatRead {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                let round = self.0.fetch_add(1, Ordering::SeqCst);
                if round < 65 {
                    return Ok(AssistantMessage {
                        tool_calls: vec![ToolCall {
                            id: format!("c{round}"),
                            kind: ToolKind::Function,
                            function: ToolFunction {
                                name: "read_file".to_string(),
                                arguments: r#"{"path":"note.txt"}"#.to_string(),
                            },
                        }],
                        ..AssistantMessage::default()
                    });
                }
                Ok(AssistantMessage {
                    content: Some("finished".to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        RepeatRead(AtomicUsize::new(0)),
        "test-model",
        &workspace,
        "test",
    )?;
    let turn = loop_.turn("keep reading").await?;
    assert_eq!(turn.rounds, 66);
    assert_eq!(turn.text, "finished");
    assert!(!turn.aborted);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn plan_mode_is_read_only_end_to_end() -> Result<()> {
    let workspace = test_workspace()?;
    struct Planner;
    impl Provider for Planner {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                Ok(AssistantMessage {
                    content: Some("here is the plan".to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }
    let mut loop_ = Loop::with_session(Planner, "test-model", &workspace, "test")?.with_plan();
    let names = loop_.tool_names();
    assert!(!names.iter().any(|name| name == "write_file"));
    let turn = loop_.turn("plan something").await?;
    assert_eq!(turn.text, "here is the plan");
    assert!(
        turn.events
            .iter()
            .any(|event| matches!(event, Event::Receipt { .. }))
    );
    assert!(
        !workspace.join(".mage/plan.md").is_file(),
        "plan mode must not own .mage/plan.md"
    );
    assert!(
        loop_
            .replay_session()
            .history
            .last()
            .is_some_and(|message| message
                .content
                .starts_with("here is the plan\n\nTurn receipt"))
    );
    let denied = loop_
        .execute_tool(&call("write_file", r#"{"path":"x.rs","content":"no"}"#))
        .await;
    denied.unwrap_err();
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn plan_inspection_can_recover_after_eight_calls() -> Result<()> {
    struct Inspector {
        rounds: AtomicUsize,
    }
    impl Provider for Inspector {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                let round = self.rounds.fetch_add(1, Ordering::SeqCst);
                if round == 8 {
                    assert!(request.messages.last().is_some_and(|message| {
                        message.content.contains("unknown tool `missing`")
                    }));
                }
                if round == 10 {
                    return Ok(AssistantMessage {
                        content: Some("final review".to_string()),
                        ..AssistantMessage::default()
                    });
                }
                Ok(AssistantMessage {
                    content: None,
                    tool_calls: vec![ToolCall {
                        id: format!("call-{round}"),
                        kind: ToolKind::Function,
                        function: ToolFunction {
                            name: if round == 7 { "missing" } else { "list_dir" }.to_string(),
                            arguments: r#"{"path":"."}"#.to_string(),
                        },
                    }],
                    ..AssistantMessage::default()
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        Inspector {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_plan();

    let turn = loop_.turn("review").await?;

    assert_eq!(turn.text, "final review");
    assert_eq!(turn.rounds, 11);
    assert_eq!(turn_receipt(&turn).searches, 9);
    assert_eq!(turn_receipt(&turn).failures.len(), 1);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn accepted_plan_executes_inside_the_selected_work() -> Result<()> {
    struct PlanThenWrite {
        rounds: AtomicUsize,
    }
    impl Provider for PlanThenWrite {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                let round = self.rounds.fetch_add(1, Ordering::SeqCst);
                Ok(match round {
                    0 => AssistantMessage {
                        content: Some("# Plan\n\n1. Write x.txt".to_string()),
                        ..AssistantMessage::default()
                    },
                    1 => AssistantMessage {
                        tool_calls: vec![call(
                            "write_file",
                            r#"{"path":"x.txt","content":"done\n"}"#,
                        )],
                        ..AssistantMessage::default()
                    },
                    _ => AssistantMessage {
                        content: Some("implemented".to_string()),
                        ..AssistantMessage::default()
                    },
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        PlanThenWrite {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_plan()
    .with_verify(vec!["true".to_string()]);

    drop(loop_.turn("plan the change").await?);
    loop_.accept_plan()?;
    loop_.set_plan(false);
    let turn = loop_.turn("implement the accepted plan").await?;

    assert!(turn.checks.iter().all(|check| check.passed));
    assert!(!turn.checks.is_empty());
    let progress = loop_.work_progress()?;
    assert_eq!(progress.kind, crate::WorkKind::Single);
    assert_eq!(progress.phase, crate::WorkPhase::Completed);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn plan_mode_rejects_job_control_at_execution() -> Result<()> {
    let workspace = test_workspace()?;
    let mut tools = crate::tools::ToolRegistry::new(&workspace)?;
    let started = tools
        .execute(&call(
            "run_command",
            r#"{"command":"sleep 1","background":true}"#,
        ))
        .await?;
    let id = started
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .ok_or_else(|| Error::Tool(format!("missing job id: {started}")))?;
    tools.set_read_only(true);
    let killed = tools
        .execute(&call(
            "run_command",
            &format!(r#"{{"job":"{id}","kill":true}}"#),
        ))
        .await;
    killed.unwrap_err();
    tools.set_read_only(false);
    tools
        .execute(&call(
            "run_command",
            &format!(r#"{{"job":"{id}","kill":true}}"#),
        ))
        .await?;
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

struct WriteThenText {
    rounds: AtomicUsize,
}
impl Provider for WriteThenText {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async {
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            Ok(match round {
                0 => AssistantMessage {
                    tool_calls: vec![call("write_file", r#"{"path":"plan.md","content":"a\n"}"#)],
                    ..AssistantMessage::default()
                },
                _ => AssistantMessage {
                    content: Some("done".to_string()),
                    ..AssistantMessage::default()
                },
            })
        })
    }
}

#[tokio::test]
async fn mutating_turn_runs_the_configured_verify_gate() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::create_dir_all(workspace.join(".mage"))?;
    std::fs::write(workspace.join(".mage/check.toml"), "check = \"false\"")?;
    let mut loop_ = Loop::with_session(
        WriteThenText {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(vec!["false".to_string()]);
    let journal = loop_.session_path().to_path_buf();
    let mut closed_before_assistant = false;
    let turn = loop_
        .turn_with("write the plan", |event| {
            if matches!(event, Event::Assistant { .. }) {
                closed_before_assistant = std::fs::read_to_string(&journal)
                    .is_ok_and(|log| log.contains(r#""kind":"close""#));
            }
        })
        .await?;
    assert!(closed_before_assistant);
    let verify = turn.checks.last().expect("mutating turn must verify");
    assert_eq!(verify.command, "false");
    assert!(!verify.passed);
    assert!(turn.events.iter().any(
        |event| matches!(event, Event::Verify { command, passed: false } if command == "false")
    ));
    assert!(turn.text.contains("done"));
    let receipt = turn_receipt(&turn);
    assert!(receipt.text.contains("Verification"));
    assert!(receipt.text.contains("failed"));
    assert!(receipt.text.contains("plan.md"));
    let replay = loop_.replay_session();
    assert_eq!(
        replay
            .history
            .last()
            .map(|message| message.content.as_str()),
        Some(crate::verify::context(&turn.text, receipt).as_str())
    );
    let log = std::fs::read_to_string(loop_.session_path())?;
    let receipt = log
        .lines()
        .find(|line| line.contains(r#""kind":"receipt""#))
        .expect("verdict must record its visible receipt");
    assert!(receipt.contains("`false` failed"), "{receipt}");
    assert_eq!(
        log.lines()
            .filter(|line| line.contains(r#""kind":"close""#))
            .count(),
        1,
        "one turn closes once"
    );
    let progress = loop_.work_progress()?;
    assert_eq!(progress.phase, crate::WorkPhase::Execute);
    assert_eq!(progress.attempts, 1);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn ordinary_turn_completes_durable_work() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        WriteThenText {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(vec!["true".to_string()]);

    let turn = loop_.turn("write the plan").await?;

    assert!(turn.checks.iter().all(|check| check.passed));
    assert!(!turn.checks.is_empty());
    assert_eq!(loop_.work_progress()?.phase, crate::WorkPhase::Completed);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn shell_writes_are_verified_and_failed_work_queues_repair() -> Result<()> {
    struct ShellThenText {
        rounds: AtomicUsize,
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
    }
    impl Provider for ShellThenText {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                self.requests
                    .lock()
                    .map_err(|_error| Error::Tool("request capture is poisoned".to_string()))?
                    .push(request.clone());
                let round = self.rounds.fetch_add(1, Ordering::SeqCst);
                Ok(if round == 0 {
                    AssistantMessage {
                        tool_calls: vec![call(
                            "run_command",
                            r#"{"command":"printf shell > shell.txt"}"#,
                        )],
                        ..AssistantMessage::default()
                    }
                } else {
                    AssistantMessage {
                        content: Some("done".to_string()),
                        ..AssistantMessage::default()
                    }
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut loop_ = Loop::with_session(
        ShellThenText {
            rounds: AtomicUsize::new(0),
            requests: requests.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(vec!["false".to_string()]);

    let turn = loop_.turn("write through the shell").await?;

    assert!(
        turn.events
            .iter()
            .any(|event| { matches!(event, Event::ToolResult { mutated: false, .. }) })
    );
    assert!(turn.checks.iter().any(|check| !check.passed));
    assert!(turn_receipt(&turn).text.contains("shell.txt"));
    let progress = loop_.work_progress()?;
    assert_eq!(progress.phase, crate::WorkPhase::Execute);
    assert_eq!(progress.attempts, 1);

    drop(loop_.turn("continue").await?);
    let requests = requests
        .lock()
        .map_err(|_error| Error::Tool("request capture is poisoned".to_string()))?;
    assert!(requests.last().is_some_and(|request| {
        request
            .messages
            .iter()
            .any(|message| message.content == "Repair the failed completion check.")
    }));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn restricted_shells_are_read_only_while_file_tools_use_the_write_list() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::create_dir_all(workspace.join(".mage"))?;
    std::fs::write(
        workspace.join(".mage/policy.toml"),
        "write = [\"allowed.txt\"]\n",
    )?;
    std::fs::write(workspace.join("forbidden.txt"), "original\n")?;
    let policy = crate::policy::Policy::discover(&workspace)?;
    let Some(_) = sandbox_for_test(&workspace)? else {
        std::fs::remove_dir_all(workspace)?;
        return Ok(());
    };
    let mut tools = crate::tools::ToolRegistry::with_policy_and_search(&workspace, policy, None)?;
    tools.enable_sandbox(false)?;

    let redirect = tools
        .execute_outcome(&call(
            "run_command",
            r#"{"command":"printf changed > forbidden.txt"}"#,
        ))
        .await?;
    assert!(!redirect.success);
    assert_eq!(
        std::fs::read_to_string(workspace.join("forbidden.txt"))?,
        "original\n"
    );

    let interpreter = tools
        .execute_outcome(&call(
            "run_command",
            r#"{"command":"python3 -c \"open('forbidden.txt','w').write('changed')\""}"#,
        ))
        .await?;
    assert!(!interpreter.success);
    assert_eq!(
        std::fs::read_to_string(workspace.join("forbidden.txt"))?,
        "original\n"
    );

    let read = tools
        .execute_outcome(&call("run_command", r#"{"command":"cat forbidden.txt"}"#))
        .await?;
    assert!(read.success);
    assert!(read.text.contains("original"));

    tools
        .execute(&call(
            "write_file",
            r#"{"path":"allowed.txt","content":"file tool\n"}"#,
        ))
        .await?;
    assert_eq!(
        std::fs::read_to_string(workspace.join("allowed.txt"))?,
        "file tool\n"
    );
    let background = match tools
        .execute_outcome(&call(
            "run_command",
            r#"{"command":"sleep 1","background":true}"#,
        ))
        .await
    {
        Ok(_) => {
            return Err(Error::Tool(
                "restricted shells started a background job".to_string(),
            ));
        }
        Err(error) => error.to_string(),
    };
    assert!(
        background.contains("restricted write policy"),
        "{background}"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn restricted_shells_fail_closed_without_a_sandbox() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::create_dir_all(workspace.join(".mage"))?;
    std::fs::write(
        workspace.join(".mage/policy.toml"),
        "write = [\"allowed.txt\"]\n",
    )?;
    let policy = crate::policy::Policy::discover(&workspace)?;
    let mut tools = crate::tools::ToolRegistry::with_policy_and_search(&workspace, policy, None)?;
    let error = match tools
        .execute_outcome(&call("run_command", r#"{"command":"cat note.txt"}"#))
        .await
    {
        Ok(_) => {
            return Err(Error::Tool(
                "restricted shell ran without a sandbox".to_string(),
            ));
        }
        Err(error) => error.to_string(),
    };
    assert!(error.contains("requires the command sandbox"), "{error}");
    tools.set_unrestricted();
    let output = tools
        .execute_outcome(&call(
            "run_command",
            r#"{"command":"printf override > outside-list.txt"}"#,
        ))
        .await?;
    assert!(output.success);
    assert_eq!(
        std::fs::read_to_string(workspace.join("outside-list.txt"))?,
        "override"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn timed_out_commands_keep_output_and_mark_partial_effects() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let result = tools
        .execute_outcome(&call(
            "run_command",
            r#"{"command":"printf timeout-diagnostic; printf partial > partial.txt; (sleep 2; printf late > late.txt) & wait","timeout_secs":1}"#,
        ))
        .await?;
    assert!(!result.success);
    assert!(result.mutated);
    assert!(
        result.text.contains("timeout-diagnostic"),
        "{}",
        result.text
    );
    assert!(
        result
            .text
            .contains("may have changed workspace files before timeout"),
        "{}",
        result.text
    );
    assert_eq!(
        std::fs::read_to_string(workspace.join("partial.txt"))?,
        "partial"
    );
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    assert!(!workspace.join("late.txt").exists());
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn consecutive_ordinary_turns_complete_separate_work() -> Result<()> {
    struct Done;
    impl Provider for Done {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                Ok(AssistantMessage {
                    content: Some("done".to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(Done, "test-model", &workspace, "test")?;

    let first = loop_.turn("first").await?;
    assert_eq!(loop_.work_progress()?.phase, crate::WorkPhase::Completed);
    let second = loop_.turn("second").await?;
    assert_eq!(loop_.work_progress()?.phase, crate::WorkPhase::Completed);
    assert_work_receipt(&first, "done");
    assert_work_receipt(&second, "done");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn mutating_turn_runs_every_green_completion_check() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        WriteThenText {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(vec!["true".to_string(), "true".to_string()]);

    let turn = loop_.turn("write the plan").await?;

    assert!(turn.checks.iter().all(|check| check.passed));
    assert!(!turn.checks.is_empty());
    assert_eq!(
        turn.events
            .iter()
            .filter(|event| matches!(event, Event::Verify { passed: true, .. }))
            .count(),
        2
    );
    assert_eq!(turn_receipt(&turn).text.matches("`true` passed").count(), 2);
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert_eq!(log.matches("`true` passed").count(), 2);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn unchanged_turn_runs_completion_checks_and_records_receipt() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        FakeProvider {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(vec!["false".to_string()]);

    let turn = loop_.turn("read the note").await?;

    assert!(turn.checks.iter().any(|check| !check.passed));
    let receipt = turn_receipt(&turn);
    assert!(receipt.text.contains("Changed: none"));
    assert!(receipt.text.contains("`false` failed"));
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert!(log.contains(r#""kind":"receipt""#));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn external_steps_turn_stays_attached_to_durable_work() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        WriteThenText {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(vec!["true".to_string()]);
    loop_.select_steps("## Steps\n\n1. Write the file", &["true".to_string()])?;

    let turn = loop_.turn("implement step 1").await?;

    assert!(turn.checks.iter().all(|check| check.passed));
    assert!(!turn.checks.is_empty());
    let progress = loop_.work_progress()?;
    assert_eq!(progress.kind, crate::WorkKind::Steps);
    assert_eq!(progress.phase, crate::WorkPhase::Completed);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn failed_external_steps_keep_the_same_work_active() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        FakeProvider {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(vec!["false".to_string()]);
    loop_.select_steps("## Steps\n\n1. Fail the check", &["false".to_string()])?;

    for attempt in 1..=5 {
        let turn = loop_
            .turn(&format!("implement step 1, attempt {attempt}"))
            .await?;
        assert!(turn.checks.iter().any(|check| !check.passed));
    }

    let progress = loop_.work_progress()?;
    assert_eq!(progress.phase, crate::WorkPhase::Execute);
    assert_eq!(progress.attempts, 5);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn goal_turn_and_terminal_result_share_durable_work() -> Result<()> {
    struct GoalProvider {
        rounds: AtomicUsize,
    }
    impl Provider for GoalProvider {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                let round = self.rounds.fetch_add(1, Ordering::SeqCst);
                assert_eq!(request.emit_deltas, round < 2);
                Ok(match round {
                    0 => AssistantMessage {
                        tool_calls: vec![call(
                            "write_file",
                            r#"{"path":"goal.txt","content":"done\n"}"#,
                        )],
                        usage: Usage {
                            prompt_tokens: 10,
                            completion_tokens: 2,
                            ..Usage::default()
                        },
                        ..AssistantMessage::default()
                    },
                    1 => AssistantMessage {
                        content: Some("implemented and verified".to_string()),
                        usage: Usage {
                            prompt_tokens: 20,
                            completion_tokens: 3,
                            ..Usage::default()
                        },
                        ..AssistantMessage::default()
                    },
                    _ => {
                        let input = &request
                            .messages
                            .last()
                            .expect("judge request must contain user evidence")
                            .content;
                        assert!(input.contains("Harness evidence:\nTurn receipt"));
                        assert!(input.contains("Changed: `goal.txt`"));
                        assert!(input.contains("Verification: `true` passed"));
                        AssistantMessage {
                            content: Some(
                                r#"{"done":true,"reason":"the requested file exists"}"#.to_string(),
                            ),
                            usage: Usage {
                                prompt_tokens: 4,
                                completion_tokens: 1,
                                ..Usage::default()
                            },
                            ..AssistantMessage::default()
                        }
                    }
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let objective = "create goal.txt";
    let checks = vec!["true".to_string()];
    let mut loop_ = Loop::with_session(
        GoalProvider {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(checks.clone());
    loop_.select_goal(objective, &checks, 0)?;
    let turn = loop_
        .turn(&crate::goal_prompt(objective, &checks, 0))
        .await?;
    assert!(turn.checks.iter().all(|check| check.passed));
    assert!(!turn.checks.is_empty());
    assert_eq!(turn.usage.prompt_tokens, 34);
    assert_eq!(turn.usage.completion_tokens, 6);
    assert_eq!(loop_.spent_tokens(), 40);

    let log = std::fs::read_to_string(loop_.session_path())?;
    assert_eq!(
        log.matches(r#""note":"goal judge""#).count(),
        1,
        "the Goal judge must be a charged provider round"
    );
    let progress = loop_.work_progress()?;
    assert_eq!(progress.kind, crate::WorkKind::Goal);
    assert_eq!(progress.phase, crate::WorkPhase::Completed);
    assert!(
        !loop_
            .replay_session()
            .history
            .iter()
            .any(|message| message.content.contains(r#"{"done":true"#)),
        "judge JSON must not enter the user conversation"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn malformed_goal_judge_keeps_repairing() -> Result<()> {
    struct MalformedJudge {
        rounds: AtomicUsize,
    }

    impl Provider for MalformedJudge {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                let round = self.rounds.fetch_add(1, Ordering::SeqCst);
                if round >= 2 {
                    let prompt = &request.messages[1].content;
                    assert!(
                        prompt.contains("Objective:\nfinish the objective"),
                        "{prompt}"
                    );
                    assert!(
                        prompt.contains("Harness evidence:\nTurn receipt"),
                        "{prompt}"
                    );
                    assert!(prompt.contains("goal.txt"), "{prompt}");
                    assert!(prompt.contains("implemented and verified"), "{prompt}");
                }
                Ok(match round {
                    0 => AssistantMessage {
                        tool_calls: vec![call(
                            "write_file",
                            r#"{"path":"goal.txt","content":"done\n"}"#,
                        )],
                        ..AssistantMessage::default()
                    },
                    1 => AssistantMessage {
                        content: Some("implemented and verified".to_string()),
                        ..AssistantMessage::default()
                    },
                    2 | 3 => AssistantMessage {
                        content: Some("looks good".to_string()),
                        ..AssistantMessage::default()
                    },
                    _ => AssistantMessage {
                        content: Some(
                            r#"{"verdict":"done","reason":"repair accepted"}"#.to_string(),
                        ),
                        ..AssistantMessage::default()
                    },
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let checks = vec!["true".to_string()];
    let mut loop_ = Loop::with_session(
        MalformedJudge {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(checks.clone());
    loop_.select_goal("finish the objective", &checks, 0)?;

    let turn = loop_.turn("finish the objective").await?;

    assert!(!turn.aborted);
    let progress = loop_.work_progress()?;
    assert_eq!(progress.phase, crate::WorkPhase::Completed);
    assert_eq!(progress.exit_code(), Some(0));
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert_eq!(log.matches(r#""note":"goal judge""#).count(), 3);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn candidate_changes_during_review_or_judging_require_rechecking() -> Result<()> {
    struct Changing {
        path: PathBuf,
        rounds: AtomicUsize,
    }
    impl Provider for Changing {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                let text = match request.model.as_str() {
                    "review" => r#"{"pass":true,"findings":""}"#,
                    "judge" => r#"{"verdict":"done","reason":"complete"}"#,
                    _ => {
                        return Ok(AssistantMessage {
                            content: Some("done".to_string()),
                            ..AssistantMessage::default()
                        });
                    }
                };
                tokio::task::yield_now().await;
                if self.rounds.fetch_add(1, Ordering::SeqCst) == 0 {
                    std::fs::write(&self.path, "changed during decision\n")?;
                }
                Ok(AssistantMessage {
                    content: Some(text.to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }

    for (role, model) in [
        (workflow::Role::Review, "review"),
        (workflow::Role::Judge, "judge"),
    ] {
        let workspace = test_workspace()?;
        let path = workspace.join("untouched.rs");
        std::fs::write(&path, "original\n")?;
        let provider = || Changing {
            path: path.clone(),
            rounds: AtomicUsize::new(0),
        };
        let mut agent = Loop::with_session(provider(), "implement", &workspace, "system")?;
        agent.set_seat(role, provider(), model, None, None);
        if role == workflow::Role::Judge {
            agent.select_goal("finish the change", &[], 3)?;
        }
        let rejected = agent.turn("finish the change").await?;
        assert_eq!(agent.work_progress()?.phase, WorkPhase::Execute);
        assert!(
            rejected.checks.iter().any(|check| !check.passed
                && check
                    .output
                    .contains("workspace changed after candidate capture")),
            "{rejected:?}"
        );
        assert!(rejected.events.iter().any(|event| matches!(event, Event::Receipt { receipt } if receipt.text.contains("workspace candidate unchanged") && receipt.text.contains("failed"))));
        let accepted = agent.turn("recheck the changed candidate").await?;
        assert!(!accepted.aborted);
        assert_eq!(agent.work_progress()?.phase, WorkPhase::Completed);
        std::fs::remove_dir_all(workspace)?;
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn work_deadline_stops_commands_checks_and_background_jobs() -> Result<()> {
    struct CommandDeadline {
        calls: Arc<AtomicUsize>,
        command: String,
        background: bool,
        verify: bool,
    }
    impl Provider for CommandDeadline {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 && !self.verify {
                    let mut write = call(
                        "write_file",
                        r#"{"path":"kept.txt","content":"completed edit\n"}"#,
                    );
                    write.id = "write".to_string();
                    return Ok(AssistantMessage {
                        tool_calls: vec![
                            write,
                            call(
                                "run_command",
                                &serde_json::json!({
                                    "command": self.command,
                                    "background": self.background,
                                })
                                .to_string(),
                            ),
                        ],
                        usage: Usage {
                            prompt_tokens: 3,
                            completion_tokens: 2,
                            ..Usage::default()
                        },
                        ..AssistantMessage::default()
                    });
                }
                Ok(AssistantMessage {
                    content: Some("done".to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }

    for (verify, background) in [(false, false), (true, false), (false, true)] {
        let workspace = test_workspace()?;
        let calls = Arc::new(AtomicUsize::new(0));
        let command =
            "echo $$ > command.pid; printf started > started.txt; sleep 30; printf late > late.txt";
        let provider = || CommandDeadline {
            calls: calls.clone(),
            command: command.to_string(),
            background,
            verify,
        };
        let mut agent = Loop::with_session(provider(), "test-model", &workspace, "system")?
            .with_budget(Budget {
                max_tokens: None,
                max_secs: Some(1),
            })?;
        if verify {
            agent = agent.with_verify(vec![command.to_string()]);
        }
        let turn = tokio::time::timeout(
            std::time::Duration::from_secs(4),
            agent.turn("bounded work"),
        )
        .await
        .expect("the Work deadline must settle the turn")?;
        assert!(turn.aborted, "{turn:?}");
        assert!(turn.text.contains("work-time limit"));
        assert_eq!(agent.work_progress()?.phase, WorkPhase::Execute);
        assert!(agent.elapsed_secs() >= 1);
        assert!(
            workspace.join("started.txt").exists(),
            "the command must have executed"
        );
        assert!(!workspace.join("late.txt").exists());
        if !verify {
            assert_eq!(
                std::fs::read_to_string(workspace.join("kept.txt"))?,
                "completed edit\n"
            );
            assert!(turn.events.iter().any(|event| matches!(event, Event::ToolResult { id, success: true, mutated: true, .. } if id == "write")), "completed results must survive later cancellation: {turn:?}");
            assert_eq!(turn.usage.prompt_tokens, 3);
            assert_eq!(turn.usage.completion_tokens, 2);
            assert!(turn.events.iter().any(
                |event| matches!(event, Event::ToolCall { name, .. } if name == "run_command")
            ));
        }
        let pid: u32 = std::fs::read_to_string(workspace.join("command.pid"))?
            .trim()
            .parse()
            .expect("shell pid");
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let state = std::process::Command::new("ps")
                    .args(["-o", "stat=", "-p", &pid.to_string()])
                    .output()?;
                let state = String::from_utf8_lossy(&state.stdout);
                if state.trim().is_empty() || state.trim().starts_with('Z') {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            Ok::<(), std::io::Error>(())
        })
        .await
        .expect("the cancelled shell must exit")?;
        let path = agent.session_path().to_path_buf();
        let count = calls.load(Ordering::SeqCst);
        drop(agent);
        let mut resumed = Loop::resume_path(provider(), "test-model", &workspace, "system", &path)?;
        if verify {
            resumed = resumed.with_verify(vec![command.to_string()]);
        }
        assert!(resumed.turn("resume").await?.aborted);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            count,
            "resume must not regain the spent time"
        );
        std::fs::remove_dir_all(workspace)?;
    }
    Ok(())
}

#[tokio::test]
async fn cancelling_a_running_tool_keeps_live_and_replayed_history_equal() -> Result<()> {
    struct SlowTool;
    impl Provider for SlowTool {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                Ok(AssistantMessage {
                    tool_calls: vec![ToolCall {
                        id: "slow".to_string(),
                        kind: ToolKind::Function,
                        function: ToolFunction {
                            name: "run_command".to_string(),
                            arguments: r#"{"command":"sleep 5"}"#.to_string(),
                        },
                    }],
                    ..AssistantMessage::default()
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(SlowTool, "test-model", &workspace, "test")?;
    let mut turn = Box::pin(loop_.turn("cancel me"));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut turn)
            .await
            .is_err(),
        "the command should still be running"
    );
    drop(turn);

    let replayed = loop_.replay_session();
    assert!(!replayed.history.is_empty());
    assert!(
        replayed
            .history
            .iter()
            .any(|message| message.content.contains("cancel me"))
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn token_budget_aborts_and_persists() -> Result<()> {
    struct Heavy;
    impl Provider for Heavy {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                Ok(AssistantMessage {
                    content: None,
                    tool_calls: vec![ToolCall {
                        id: "c".to_string(),
                        kind: ToolKind::Function,
                        function: ToolFunction {
                            name: "read_file".to_string(),
                            arguments: r#"{"path":"note.txt"}"#.to_string(),
                        },
                    }],
                    reasoning_content: None,
                    usage: crate::provider::Usage {
                        prompt_tokens: 900,
                        completion_tokens: 100,
                        reasoning_tokens: 0,
                        cached_tokens: 300,
                    },
                    stop_reason: StopReason::ToolCalls,
                })
            })
        }
    }
    let workspace = test_workspace()?;
    let mut loop_ =
        Loop::with_session(Heavy, "test-model", &workspace, "test")?.with_budget(Budget {
            max_tokens: Some(1000),
            max_secs: None,
        })?;
    let turn = loop_.turn("go").await?;
    assert!(turn.aborted);
    assert!(turn.text.contains("token limit"));
    assert!(loop_.history_len() > 0);
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert!(!log.is_empty());
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn resumed_budget_uses_durable_usage_and_ceiling() -> Result<()> {
    struct Counted {
        calls: std::sync::Arc<AtomicUsize>,
    }
    impl Provider for Counted {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            let calls = self.calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(AssistantMessage {
                    content: Some("done".to_string()),
                    usage: crate::provider::Usage {
                        prompt_tokens: 100,
                        completion_tokens: 0,
                        reasoning_tokens: 0,
                        cached_tokens: 0,
                    },
                    ..AssistantMessage::default()
                })
            })
        }
    }
    let workspace = test_workspace()?;
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let mut first = Loop::with_session(
        Counted {
            calls: calls.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_budget(Budget {
        max_tokens: Some(100),
        max_secs: None,
    })?;
    let path = first.session_path().to_path_buf();
    drop(first.turn("first").await?);
    assert_eq!(first.spent_tokens(), 100);
    drop(first);

    let mut resumed = Loop::resume_path(
        Counted {
            calls: calls.clone(),
        },
        "test-model",
        &workspace,
        "test",
        &path,
    )?;
    assert_eq!(resumed.spent_tokens(), 100);
    assert_eq!(resumed.budget().max_tokens, Some(100));
    let turn = resumed.turn("resume").await?;
    assert!(turn.aborted);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(turn.text.contains("token limit"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn failed_provider_consumes_durable_budget_before_retry() -> Result<()> {
    struct Failing {
        calls: std::sync::Arc<AtomicUsize>,
    }
    impl Provider for Failing {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            let calls = self.calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(Error::provider(
                    ProviderFailureKind::Authentication,
                    "denied",
                ))
            })
        }
    }

    let workspace = test_workspace()?;
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let mut first = Loop::with_session(
        Failing {
            calls: calls.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_budget(Budget {
        max_tokens: Some(1),
        max_secs: None,
    })?;
    let path = first.session_path().to_path_buf();
    let failed = first.turn("first").await?;
    assert!(failed.aborted);
    assert!(failed.text.contains("token limit"));
    assert!(first.spent_tokens() > 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    drop(first);

    let mut resumed = Loop::resume_path(
        Failing {
            calls: calls.clone(),
        },
        "test-model",
        &workspace,
        "test",
        &path,
    )?;
    let turn = resumed.turn("second").await?;
    assert!(turn.aborted);
    assert!(turn.text.contains("token limit"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn resumed_time_budget_uses_durable_live_time() -> Result<()> {
    struct Slow {
        calls: std::sync::Arc<AtomicUsize>,
    }
    impl Provider for Slow {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            let calls = self.calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
                Ok(AssistantMessage {
                    content: Some("done".to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }
    let workspace = test_workspace()?;
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let mut first = Loop::with_session(
        Slow {
            calls: calls.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_budget(Budget {
        max_tokens: None,
        max_secs: Some(1),
    })?;
    let path = first.session_path().to_path_buf();
    let started = std::time::Instant::now();
    let stopped = first.turn("first").await?;
    assert!(stopped.aborted);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert!(
        stopped
            .events
            .iter()
            .any(|event| { matches!(event, Event::Receipt { receipt } if receipt.aborted) })
    );
    assert!(std::fs::read_to_string(&path)?.contains(r#""kind":"receipt""#));
    drop(first);

    let mut resumed = Loop::resume_path(
        Slow {
            calls: calls.clone(),
        },
        "test-model",
        &workspace,
        "test",
        &path,
    )?;
    assert_eq!(resumed.budget().max_secs, Some(1));
    assert!(resumed.elapsed_secs() >= 1);
    let turn = resumed.turn("resume").await?;
    assert!(turn.aborted);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(turn.text.contains("work-time limit"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn consumed_budget_blocks_the_next_turn_before_provider_call() -> Result<()> {
    struct OneRound {
        calls: std::sync::Arc<AtomicUsize>,
    }
    impl Provider for OneRound {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            let calls = self.calls.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(AssistantMessage {
                    content: Some("done".to_string()),
                    usage: crate::provider::Usage {
                        prompt_tokens: 100,
                        completion_tokens: 0,
                        reasoning_tokens: 0,
                        cached_tokens: 0,
                    },
                    ..AssistantMessage::default()
                })
            })
        }
    }
    let workspace = test_workspace()?;
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let mut loop_ = Loop::with_session(
        OneRound {
            calls: calls.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_budget(Budget {
        max_tokens: Some(100),
        max_secs: None,
    })?;
    assert_work_receipt(&loop_.turn("first").await?, "done");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let second = loop_.turn("second").await?;
    assert!(second.aborted);
    assert!(second.text.contains("token limit"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[test]
fn system_prompt_falls_back_without_local_files() -> Result<()> {
    let workspace = test_workspace()?;
    let prompt = system_prompt(&workspace);
    assert!(
        prompt.len() < 700,
        "default prompt is {} chars",
        prompt.len()
    );
    assert!(!prompt.contains("You are Mage"));
    assert!(!prompt.contains("general-purpose agent"));
    assert!(!prompt.contains("web_search"));
    assert!(!prompt.contains("web_fetch"));
    assert!(prompt.contains("Keep reasoning brief"));
    assert!(prompt.contains("Stop when the task is proved"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[test]
fn agents_md_loads_only_when_trusted() -> Result<()> {
    let workspace = test_workspace()?;
    std::fs::write(workspace.join("AGENTS.md"), "UNIQUE_AGENTS_MARK")?;
    std::fs::write(workspace.join("SYSTEM.md"), "LOCAL_SYSTEM_MARK")?;
    let home = workspace.join("home");
    std::fs::create_dir_all(&home)?;
    std::fs::write(home.join("SYSTEM.md"), "HOME_SYSTEM_MARK")?;
    let closed = crate::prompt::compose(&workspace, Some(&home), false, true);
    assert!(closed.contains("HOME_SYSTEM_MARK"));
    assert!(closed.contains("LOCAL_SYSTEM_MARK"));
    assert!(!closed.contains("UNIQUE_AGENTS_MARK"));
    let open = crate::prompt::compose(&workspace, Some(&home), true, true);
    assert!(open.contains("UNIQUE_AGENTS_MARK"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn set_plan_switches_schema_and_enforces_policy() -> Result<()> {
    let workspace = test_workspace()?;
    struct Idle;
    impl Provider for Idle {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                Ok(AssistantMessage {
                    content: Some("ok".to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }
    let mut loop_ = Loop::with_session(Idle, "test-model", &workspace, "test")?;
    assert!(!loop_.is_plan());
    assert!(loop_.tool_names().contains(&"write_file".to_string()));
    loop_.set_plan(true);
    assert!(loop_.is_plan());
    let names = loop_.tool_names();
    assert!(!names.contains(&"write_file".to_string()));
    assert!(names.contains(&"run_command".to_string()));
    loop_.set_plan(false);
    assert!(loop_.tool_names().contains(&"write_file".to_string()));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn mode_notes_preserve_the_request_prefix() -> Result<()> {
    struct Capture(Arc<Mutex<Vec<CompletionRequest>>>);
    impl Provider for Capture {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                if let Ok(mut requests) = self.0.lock() {
                    requests.push(request.clone());
                }
                Ok(AssistantMessage {
                    content: Some("ok".to_string()),
                    usage: Usage {
                        prompt_tokens: 10,
                        completion_tokens: 1,
                        reasoning_tokens: 0,
                        cached_tokens: 5,
                    },
                    ..AssistantMessage::default()
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut loop_ = Loop::with_session(
        Capture(requests.clone()),
        "test-model",
        &workspace,
        "BASE\n\nSKILLS-SNAPSHOT",
    )?
    .with_verify(vec!["true".to_string()]);

    let first = loop_.turn("auto one").await?;
    loop_.set_plan(true);
    loop_.turn("plan").await?;
    loop_.accept_plan()?;
    loop_.set_plan(false);
    loop_.turn("auto two").await?;

    let requests = requests
        .lock()
        .map_err(|_error| Error::Tool("request capture is poisoned".to_string()))?;
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| request.messages[0].content == "BASE\n\nSKILLS-SNAPSHOT")
    );
    assert_eq!(requests[0].prefix_hash(), requests[2].prefix_hash());
    assert_ne!(requests[0].prefix_hash(), requests[1].prefix_hash());
    assert!(requests[0].messages.last().is_some_and(|message| {
        message.content.contains("completion check") && !message.content.contains("plan mode")
    }));
    assert!(requests[1].messages.last().is_some_and(|message| {
        message.content.contains("completion check") && message.content.contains("plan mode")
    }));
    assert!(first.events.iter().any(|event| matches!(
        event,
        Event::Usage { prefix_hash, .. } if prefix_hash == &requests[0].prefix_hash()
    )));
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert!(log.contains("auto one"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn a_review_seat_gates_the_turn_and_sends_findings_back() -> Result<()> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Part {
        Implementer,
        Reviewer,
    }
    struct Cast {
        part: Part,
        rounds: AtomicUsize,
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
    }
    impl Provider for Cast {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                if let Ok(mut requests) = self.requests.lock() {
                    requests.push(request.clone());
                }
                let round = self.rounds.fetch_add(1, Ordering::SeqCst);
                Ok(match self.part {
                    Part::Implementer if round == 0 => AssistantMessage {
                        tool_calls: vec![call(
                            "write_file",
                            r#"{"path":"plan.md","content":"a\n"}"#,
                        )],
                        ..AssistantMessage::default()
                    },
                    Part::Implementer => AssistantMessage {
                        content: Some("done".to_string()),
                        ..AssistantMessage::default()
                    },
                    Part::Reviewer => {
                        assert!(request.messages[0].content.starts_with("You review"));
                        let answered = request
                            .messages
                            .iter()
                            .any(|message| message.role == Role::Tool);
                        if answered {
                            let verdict = if round < 2 {
                                r#"{"pass":false,"findings":"add a test for plan.md"}"#
                            } else {
                                r#"{"pass":true,"findings":""}"#
                            };
                            AssistantMessage {
                                content: Some(verdict.to_string()),
                                ..AssistantMessage::default()
                            }
                        } else {
                            AssistantMessage {
                                tool_calls: vec![call(
                                    "write_file",
                                    r#"{"path":"reviewer.txt","content":"no"}"#,
                                )],
                                ..AssistantMessage::default()
                            }
                        }
                    }
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut loop_ = Loop::with_session(
        Cast {
            part: Part::Implementer,
            rounds: AtomicUsize::new(0),
            requests: requests.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(vec!["true".to_string()]);
    loop_.set_seat(
        crate::workflow::Role::Review,
        Cast {
            part: Part::Reviewer,
            rounds: AtomicUsize::new(0),
            requests: requests.clone(),
        },
        "review-model",
        None,
        None,
    );

    let first = loop_.turn("write the plan").await?;
    let review = first.checks.last().expect("the review is the last check");
    assert_eq!(review.command, "review");
    assert!(!review.passed);
    let receipt = turn_receipt(&first);
    assert!(receipt.text.contains("`review` failed"), "{}", receipt.text);
    assert!(
        receipt.text.contains("add a test for plan.md"),
        "{}",
        receipt.text
    );
    assert!(first.events.iter().any(
        |event| matches!(event, Event::Verify { command, passed: false } if command == "review")
    ));
    let progress = loop_.work_progress()?;
    assert_eq!(progress.phase, crate::WorkPhase::Execute);
    assert_eq!(progress.attempts, 1);
    assert!(
        !workspace.join("reviewer.txt").exists(),
        "the reviewer's write must be denied"
    );

    let second = loop_.turn("continue").await?;
    assert!(
        second
            .checks
            .last()
            .is_some_and(|check| check.command == "review" && check.passed)
    );
    assert_eq!(loop_.work_progress()?.phase, crate::WorkPhase::Completed);

    let seen = requests
        .lock()
        .map_err(|_error| Error::Tool("request capture is poisoned".to_string()))?
        .clone();
    let implementer: Vec<&CompletionRequest> = seen
        .iter()
        .filter(|request| request.messages[0].content == "test")
        .collect();
    assert!(
        implementer[0]
            .messages
            .last()
            .is_some_and(|message| { message.content.contains("a reviewer reads the change") })
    );
    assert!(implementer.last().is_some_and(|request| {
        request.messages.iter().any(|message| {
            message.content == "Address the review findings:\nadd a test for plan.md"
        })
    }));
    let history = loop_.replay_session().history;
    assert!(
        !history
            .iter()
            .any(|message| message.content.contains("\"pass\"")),
        "reviewer chatter stays out of the conversation"
    );
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert!(log.matches(r#""note":"review""#).count() >= 2, "{log}");
    assert!(log.contains(r#""model":"review-model""#));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn an_unavailable_review_fails_completion_and_keeps_work_resumable() -> Result<()> {
    enum Part {
        Implementer(AtomicUsize),
        Reviewer,
    }
    struct ReviewProvider(Part);
    impl Provider for ReviewProvider {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                match &self.0 {
                    Part::Implementer(rounds) => {
                        if rounds.fetch_add(1, Ordering::SeqCst) == 0 {
                            Ok(AssistantMessage {
                                tool_calls: vec![call(
                                    "write_file",
                                    r#"{"path":"reviewed.txt","content":"done\n"}"#,
                                )],
                                ..AssistantMessage::default()
                            })
                        } else {
                            Ok(AssistantMessage {
                                content: Some("done".to_string()),
                                ..AssistantMessage::default()
                            })
                        }
                    }
                    Part::Reviewer => Err(Error::provider(
                        ProviderFailureKind::Authentication,
                        "HTTP 401",
                    )),
                }
            })
        }
    }

    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        ReviewProvider(Part::Implementer(AtomicUsize::new(0))),
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(vec!["true".to_string()]);
    loop_.set_seat(
        crate::workflow::Role::Review,
        ReviewProvider(Part::Reviewer),
        "review-model",
        None,
        None,
    );

    let turn = loop_.turn("write the file").await?;
    let review = turn
        .checks
        .last()
        .ok_or_else(|| Error::Config("unavailable review did not produce a check".to_string()))?;
    assert_eq!(review.command, "review");
    assert!(!review.passed);
    assert!(review.output.contains("review unavailable"));
    assert!(turn.events.iter().any(
        |event| matches!(event, Event::Verify { command, passed: false } if command == "review")
    ));
    assert!(turn_receipt(&turn).text.contains("`review` failed"));
    assert_eq!(loop_.work_progress()?.phase, crate::WorkPhase::Execute);
    assert!(!loop_.work_progress()?.terminal());
    assert!(workspace.join("reviewed.txt").is_file());
    let retry = loop_.turn("continue").await?;
    assert!(
        retry
            .checks
            .last()
            .is_some_and(|check| check.command == "review" && !check.passed)
    );
    assert_eq!(loop_.work_progress()?.phase, crate::WorkPhase::Execute);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn a_scout_seat_briefs_job_work_once_and_never_chat_turns() -> Result<()> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Part {
        Implementer,
        Scout,
    }
    struct Cast {
        part: Part,
        rounds: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
    }
    impl Provider for Cast {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                if let Ok(mut requests) = self.requests.lock() {
                    requests.push(request.clone());
                }
                self.rounds.fetch_add(1, Ordering::SeqCst);
                Ok(match self.part {
                    Part::Implementer => AssistantMessage {
                        content: Some("done".to_string()),
                        ..AssistantMessage::default()
                    },
                    Part::Scout => {
                        assert!(request.messages[0].content.starts_with("You scout"));
                        AssistantMessage {
                            content: Some("touches note.txt; no helpers".to_string()),
                            ..AssistantMessage::default()
                        }
                    }
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let scouted = Arc::new(AtomicUsize::new(0));
    let mut loop_ = Loop::with_session(
        Cast {
            part: Part::Implementer,
            rounds: Arc::new(AtomicUsize::new(0)),
            requests: requests.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(vec!["true".to_string()]);
    loop_.set_seat(
        crate::workflow::Role::Scout,
        Cast {
            part: Part::Scout,
            rounds: scouted.clone(),
            requests: requests.clone(),
        },
        "scout-model",
        None,
        None,
    );

    drop(loop_.turn("just chat").await?);
    assert_eq!(scouted.load(Ordering::SeqCst), 0, "chat turns never scout");

    loop_.select_steps("## Steps\n\n1. First.\n2. Second.", &["true".to_string()])?;
    let first = loop_.turn("implement step 1").await?;
    assert_eq!(scouted.load(Ordering::SeqCst), 1);
    assert!(
        first
            .events
            .iter()
            .any(|event| matches!(event, Event::Note { text } if text.contains("scout brief")))
    );
    drop(loop_.turn("implement step 2").await?);
    assert_eq!(scouted.load(Ordering::SeqCst), 1, "one brief per Work");
    assert_eq!(loop_.work_progress()?.phase, crate::WorkPhase::Completed);

    let seen = requests
        .lock()
        .map_err(|_error| Error::Tool("request capture is poisoned".to_string()))?
        .clone();
    let step_one = seen
        .iter()
        .find(|request| {
            request
                .messages
                .iter()
                .any(|message| message.content.starts_with("implement step 1"))
        })
        .expect("the step 1 request");
    let position = step_one
        .messages
        .iter()
        .position(|message| message.content == "Scout brief:\ntouches note.txt; no helpers")
        .expect("the brief follows the input");
    assert!(
        step_one.messages[position - 1]
            .content
            .starts_with("implement step 1")
    );
    let history = loop_.replay_session().history;
    assert_eq!(
        history
            .iter()
            .filter(|message| message.content.starts_with("Scout brief:"))
            .count(),
        1
    );
    let work = std::fs::read_to_string(loop_.session_path().with_file_name("work.json"))?;
    assert!(
        work.contains("\"brief\": \"touches note.txt; no helpers\""),
        "{work}"
    );
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert!(log.contains(r#""note":"scout""#));
    assert!(log.contains(r#""model":"scout-model""#));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn resume_binds_tools_to_the_recorded_workspace() -> Result<()> {
    struct Idle;
    impl Provider for Idle {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                Ok(AssistantMessage {
                    content: Some("ok".to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }
    let workspace = test_workspace()?;
    let elsewhere = test_workspace()?;
    let first = Loop::with_session(Idle, "test-model", &workspace, "test")?;
    let path = first.session_path().to_path_buf();
    drop(first);

    let resumed = Loop::resume_path(Idle, "test-model", &elsewhere, "test", &path)?;
    assert_eq!(
        resumed.workspace().canonicalize()?,
        workspace.canonicalize()?,
        "tools run where the session was recorded, not where it was resumed"
    );
    std::fs::remove_dir_all(workspace)?;
    std::fs::remove_dir_all(elsewhere)?;
    Ok(())
}

#[tokio::test]
async fn goal_continuation_turns_restate_the_objective() -> Result<()> {
    struct Continue(Arc<Mutex<Vec<CompletionRequest>>>);

    impl Provider for Continue {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                let judge = request.messages[0].content.starts_with("You judge");
                if !judge {
                    self.0
                        .lock()
                        .map_err(|_error| Error::Tool("capture is poisoned".to_string()))?
                        .push(request.clone());
                }
                Ok(AssistantMessage {
                    content: Some(if judge {
                        r#"{"verdict":"continue","reason":"more to do"}"#.to_string()
                    } else {
                        "worked on it".to_string()
                    }),
                    ..AssistantMessage::default()
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let objective = "EXACT-OBJECTIVE-4821: make the widget spin";
    let checks = vec!["true".to_string()];
    let mut loop_ =
        Loop::with_session(Continue(requests.clone()), "test-model", &workspace, "test")?
            .with_verify(checks.clone());
    loop_.select_goal(objective, &checks, 0)?;
    loop_
        .turn(&crate::goal_prompt(objective, &checks, 0))
        .await?;
    let rounds = loop_.work_progress()?.rounds;
    loop_
        .turn(&crate::goal_prompt(objective, &checks, rounds))
        .await?;
    let captured = requests
        .lock()
        .map_err(|_error| Error::Tool("capture is poisoned".to_string()))?
        .clone();
    assert_eq!(captured.len(), 2);
    let second = &captured[1];
    let opener = second
        .messages
        .iter()
        .rev()
        .find(|message| {
            message.role == Role::User && message.content.contains("[summon goal: continue")
        })
        .expect("the continuation opener");
    assert!(
        opener.content.contains("EXACT-OBJECTIVE-4821"),
        "the harness note restates the objective: {}",
        opener.content
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}
