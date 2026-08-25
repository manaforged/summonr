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
    assert!(names.iter().any(|name| name == "write_file"));
    let turn = loop_.turn("plan something").await?;
    assert_eq!(turn.text, "here is the plan");
    assert!(
        !workspace.join(".mage/plan.md").is_file(),
        "plan mode must not own .mage/plan.md"
    );
    assert_eq!(
        loop_
            .replay_session()
            .history
            .last()
            .map(|message| message.content.as_str()),
        Some("here is the plan")
    );
    loop_.shell("echo hi").await.unwrap_err();
    let denied = loop_
        .execute_tool(&call("write_file", r#"{"path":"x.rs","content":"no"}"#))
        .await;
    denied.unwrap_err();
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

    assert!(turn.verify.as_ref().is_some_and(|check| check.passed));
    assert!(turn.text.contains("Turn receipt"));
    let state = crate::durable::journal::inspect(loop_.session_path())
        .map_err(|error| Error::Config(error.to_string()))?;
    assert_eq!(state.works.len(), 1);
    let work = state
        .works
        .values()
        .next()
        .expect("accepted Plan must own Work");
    assert!(matches!(
        work.status(),
        crate::durable::work::WorkStatus::Completed { .. }
    ));
    assert!(work.candidate().is_some());
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn failed_plan_persistence_keeps_live_and_replayed_history_equal() -> Result<()> {
    struct Planner;
    impl Provider for Planner {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                Ok(AssistantMessage {
                    content: Some("draft".to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(Planner, "test-model", &workspace, "test")?.with_plan();
    drop(std::fs::remove_dir_all(workspace.join(".mage")));
    std::fs::write(workspace.join(".mage"), "not a directory")?;

    loop_.turn("plan something").await.unwrap_err();
    let replayed = loop_.replay_session();
    assert_eq!(replayed.history.len(), loop_.history_len());
    assert_eq!(replayed.history.len(), 1);
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
                    .is_ok_and(|log| log.contains(r#""fact":"operation_closed""#));
            }
        })
        .await?;
    assert!(closed_before_assistant);
    let verify = turn.verify.as_ref().expect("mutating turn must verify");
    assert_eq!(verify.command, "false");
    assert!(!verify.passed);
    assert!(turn.events.iter().any(
        |event| matches!(event, Event::Verify { command, passed: false } if command == "false")
    ));
    assert!(turn.text.contains("done"));
    assert!(turn.text.contains("Verification"));
    assert!(turn.text.contains("failed"));
    assert!(turn.text.contains("plan.md"));
    let replay = loop_.replay_session();
    assert_eq!(
        replay
            .history
            .last()
            .map(|message| message.content.as_str()),
        Some(turn.text.as_str())
    );
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert!(log.contains(r#""fact":"completion_check_recorded""#));
    assert!(log.contains(r#""passed":false"#));
    assert!(log.contains(r#""outcome":"failed""#));
    let execute_close = log
        .lines()
        .find(|line| {
            line.contains(r#""fact":"operation_closed""#)
                && line.contains(r#""stage_outcome":"advanced""#)
        })
        .expect("turn must close its operation");
    assert!(!execute_close.contains(r#""provider":null"#));
    let verdict_close = log
        .lines()
        .find(|line| {
            line.contains(r#""fact":"operation_closed""#)
                && line.contains(r#""stage_outcome":"returned""#)
        })
        .expect("verification must close its operation");
    assert!(
        verdict_close.contains(r#""source":"receipt""#),
        "verdict and visible receipt must share one append: {verdict_close}"
    );
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

    assert!(turn.verify.as_ref().is_some_and(|check| check.passed));
    let state = crate::durable::journal::inspect(loop_.session_path())
        .map_err(|error| Error::Config(error.to_string()))?;
    assert_eq!(state.works.len(), 1);
    let work = state.works.values().next().expect("turn must create Work");
    assert!(matches!(
        work.status(),
        crate::durable::work::WorkStatus::Completed { .. }
    ));
    assert!(work.candidate().is_some());
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
    assert!(turn.verify.as_ref().is_some_and(|check| !check.passed));
    assert!(turn.text.contains("shell.txt"));
    let state = crate::durable::journal::inspect(loop_.session_path())
        .map_err(|error| Error::Config(error.to_string()))?;
    let work = state
        .works
        .values()
        .find(|work| !work.status().terminal())
        .expect("failed Work must remain active");
    assert_eq!(
        work.status(),
        &crate::durable::work::WorkStatus::Active(crate::durable::work::WorkStage::Execute)
    );
    assert!(work.candidate().is_none());

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

    drop(loop_.turn("first").await?);
    drop(loop_.turn("second").await?);

    let state = crate::durable::journal::inspect(loop_.session_path())
        .map_err(|error| Error::Config(error.to_string()))?;
    assert_eq!(state.works.len(), 2);
    assert!(state.works.values().all(|work| matches!(
        work.status(),
        crate::durable::work::WorkStatus::Completed { .. }
    )));
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

    assert!(turn.verify.as_ref().is_some_and(|check| check.passed));
    assert_eq!(
        turn.events
            .iter()
            .filter(|event| matches!(event, Event::Verify { passed: true, .. }))
            .count(),
        2
    );
    assert_eq!(turn.text.matches("`true` passed").count(), 2);
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert_eq!(
        log.matches(r#""fact":"completion_check_recorded""#).count(),
        2
    );
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

    assert!(turn.verify.as_ref().is_some_and(|check| !check.passed));
    assert!(turn.text.contains("Turn receipt"));
    assert!(turn.text.contains("Changed: none"));
    assert!(turn.text.contains("`false` failed"));
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert!(log.contains(r#""fact":"completion_check_recorded""#));
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

    assert!(turn.verify.as_ref().is_some_and(|check| check.passed));
    assert!(turn.text.contains("Turn receipt"));
    let state = crate::durable::journal::inspect(loop_.session_path())
        .map_err(|error| Error::Config(error.to_string()))?;
    let work = state
        .works
        .values()
        .next()
        .expect("step mode must own Work");
    assert!(matches!(
        work.status(),
        crate::durable::work::WorkStatus::Completed { .. }
    ));
    assert!(work.candidate().is_some());
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn failed_external_steps_abandon_the_same_work() -> Result<()> {
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

    for attempt in 1..=3 {
        let turn = loop_
            .turn(&format!("implement step 1, attempt {attempt}"))
            .await?;
        assert!(turn.verify.as_ref().is_some_and(|check| !check.passed));
    }

    let state = crate::durable::journal::inspect(loop_.session_path())
        .map_err(|error| Error::Config(error.to_string()))?;
    let work = state
        .works
        .values()
        .next()
        .expect("step mode must own Work");
    assert!(matches!(
        work.status(),
        crate::durable::work::WorkStatus::Abandoned { .. }
    ));
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
                    _ => AssistantMessage {
                        content: Some(
                            r#"{"done":true,"reason":"the requested file exists"}"#.to_string(),
                        ),
                        usage: Usage {
                            prompt_tokens: 4,
                            completion_tokens: 1,
                            ..Usage::default()
                        },
                        ..AssistantMessage::default()
                    },
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
    loop_.select_goal(objective, &checks, crate::DEFAULT_MAX_TURNS)?;
    let turn = loop_
        .turn(&crate::goal_prompt(objective, &checks, 0))
        .await?;
    assert!(turn.verify.as_ref().is_some_and(|check| check.passed));
    assert_eq!(turn.usage.prompt_tokens, 34);
    assert_eq!(turn.usage.completion_tokens, 6);
    assert_eq!(loop_.spent_tokens(), 40);

    let state = crate::durable::journal::inspect(loop_.session_path())
        .map_err(|error| Error::Config(error.to_string()))?;
    assert_eq!(
        state
            .providers
            .values()
            .filter(|provider| matches!(
                provider,
                crate::durable::reducer::ProviderState::Finished { intent, .. }
                    if intent.purpose == crate::durable::effect::ProviderPurpose::GoalJudge
            ))
            .count(),
        1,
        "the Goal judge must be a durable provider effect"
    );
    let work = state
        .works
        .values()
        .next()
        .expect("goal mode must own Work");
    assert!(matches!(
        work.policy(),
        crate::durable::work::WorkPolicy::Goal { .. }
    ));
    assert!(matches!(
        work.status(),
        crate::durable::work::WorkStatus::Completed { .. }
    ));
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
    drop(first.turn("first").await?);
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
    assert!(turn.text.contains("wall-clock limit"));
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
    let closed = crate::prompt::compose(&workspace, Some(&home), false);
    assert!(closed.contains("HOME_SYSTEM_MARK"));
    assert!(closed.contains("LOCAL_SYSTEM_MARK"));
    assert!(!closed.contains("UNIQUE_AGENTS_MARK"));
    let open = crate::prompt::compose(&workspace, Some(&home), true);
    assert!(open.contains("UNIQUE_AGENTS_MARK"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn set_plan_keeps_schema_and_enforces_policy() -> Result<()> {
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
    let automatic = loop_.tool_names();
    loop_.set_plan(true);
    assert!(loop_.is_plan());
    let names = loop_.tool_names();
    assert_eq!(names, automatic);
    loop_.shell("echo hi").await.unwrap_err();
    loop_.set_plan(false);
    assert!(loop_.tool_names().contains(&"write_file".to_string()));
    loop_.shell("echo hi").await.unwrap();
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
    assert!(
        requests
            .iter()
            .all(|request| request.prefix_hash() == requests[0].prefix_hash())
    );
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
