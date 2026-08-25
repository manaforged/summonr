use super::*;

struct SchemaCapture(Arc<Mutex<Vec<CompletionRequest>>>);

impl Provider for SchemaCapture {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            self.0
                .lock()
                .map_err(|_error| Error::Tool("schema capture is poisoned".to_string()))?
                .push(request.clone());
            Ok(AssistantMessage {
                content: Some("ok".to_string()),
                ..AssistantMessage::default()
            })
        })
    }
}

fn captured(requests: &Arc<Mutex<Vec<CompletionRequest>>>) -> Result<Vec<CompletionRequest>> {
    requests
        .lock()
        .map(|requests| requests.clone())
        .map_err(|_error| Error::Tool("schema capture is poisoned".to_string()))
}

fn schema_loop(
    workspace: &std::path::Path,
    requests: Arc<Mutex<Vec<CompletionRequest>>>,
) -> Result<Loop<SchemaCapture>> {
    Loop::with_session(SchemaCapture(requests), "test-model", workspace, "system")
}

#[tokio::test]
async fn snapped_schema_survives_plan_toggle() -> Result<()> {
    let workspace = test_workspace()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut loop_ = schema_loop(&workspace, requests.clone())?;

    loop_.turn("first").await?;
    loop_.set_plan(true);
    loop_.turn("second").await?;
    loop_.accept_plan()?;
    loop_.set_plan(false);
    loop_.turn("third").await?;

    let requests = captured(&requests)?;
    assert_eq!(requests.len(), 3);
    let schemas = requests
        .iter()
        .map(|request| serde_json::to_vec(&request.tools))
        .collect::<serde_json::Result<Vec<_>>>()?;
    assert!(schemas.iter().all(|schema| schema == &schemas[0]));
    assert!(
        requests
            .iter()
            .all(|request| request.messages[0] == requests[0].messages[0])
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn fork_keeps_the_provider_schema() -> Result<()> {
    let workspace = test_workspace()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut loop_ = schema_loop(&workspace, requests.clone())?;

    loop_.turn("first").await?;
    drop(loop_.fork_live()?);
    loop_.turn("second").await?;

    let requests = captured(&requests)?;
    assert_eq!(requests.len(), 2);
    assert_eq!(
        serde_json::to_vec(&requests[0].tools)?,
        serde_json::to_vec(&requests[1].tools)?
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[test]
fn harness_section_lists_plan_and_check() {
    let both = crate::prompt::harness_section(true, &["cargo test".to_string()]);
    assert!(both.contains("plan mode"));
    assert!(both.contains("cargo test"));
    let automatic = crate::prompt::harness_section(false, &[]);
    assert!(automatic.contains("implementation mode"));
    assert!(automatic.contains("no completion check"));
}

#[tokio::test]
async fn resume_replays_tool_history() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        FakeProvider {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?;
    drop(loop_.turn("read the note").await?);
    let replayed = loop_.replay_session();
    assert!(replayed.history.iter().any(|message| {
        message
            .tool_calls
            .iter()
            .any(|call| call.function.name == "read_file")
    }));
    assert!(
        replayed
            .history
            .iter()
            .any(|message| message.reasoning.as_deref() == Some("inspect the file"))
    );
    assert!(
        replayed
            .history
            .iter()
            .any(|message| message.reasoning.as_deref() == Some("answer from the result"))
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn clear_history_is_durable() -> Result<()> {
    struct Text;
    impl Provider for Text {
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
    let mut loop_ = Loop::with_session(Text, "test-model", &workspace, "test")?;
    loop_.turn("forget this").await?;
    loop_.clear_history()?;
    assert_eq!(loop_.history_len(), 0);
    assert!(loop_.replay_session().history.is_empty());
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn two_tool_round_journals_every_call() -> Result<()> {
    struct TwoReads {
        rounds: AtomicUsize,
    }
    impl Provider for TwoReads {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                if self.rounds.fetch_add(1, Ordering::SeqCst) == 0 {
                    Ok(AssistantMessage {
                        content: None,
                        tool_calls: vec![
                            ToolCall {
                                id: "c1".to_string(),
                                kind: ToolKind::Function,
                                function: ToolFunction {
                                    name: "read_file".to_string(),
                                    arguments: r#"{"path":"note.txt"}"#.to_string(),
                                },
                            },
                            ToolCall {
                                id: "c2".to_string(),
                                kind: ToolKind::Function,
                                function: ToolFunction {
                                    name: "read_file".to_string(),
                                    arguments: r#"{"path":"note.txt"}"#.to_string(),
                                },
                            },
                        ],
                        ..Default::default()
                    })
                } else {
                    Ok(AssistantMessage {
                        content: Some("done".to_string()),
                        ..Default::default()
                    })
                }
            })
        }
    }
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        TwoReads {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?;
    let turn = loop_.turn("read twice").await?;
    assert_work_receipt(&turn, "done");
    let replayed = loop_.replay_session();
    let tools: Vec<_> = replayed
        .history
        .iter()
        .filter(|message| {
            message
                .tool_calls
                .iter()
                .any(|call| call.function.name == "read_file")
        })
        .collect();
    assert_eq!(tools.len(), 1, "one assistant tool batch");
    assert_eq!(tools[0].tool_calls.len(), 2, "both calls survive replay");
    assert_eq!(
        replayed
            .history
            .iter()
            .filter(|message| message.role == Role::Tool)
            .count(),
        2,
        "both tool results survive replay"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn steering_lands_before_the_next_round() -> Result<()> {
    type Seen = std::sync::Arc<std::sync::Mutex<Vec<String>>>;
    struct Recorder {
        rounds: AtomicUsize,
        seen: Seen,
    }
    impl Provider for Recorder {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                for message in &request.messages {
                    if let Ok(mut seen) = self.seen.lock() {
                        seen.push(message.content.clone());
                    }
                }
                let round = self.rounds.fetch_add(1, Ordering::SeqCst);
                if round == 0 {
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
                } else {
                    Ok(AssistantMessage {
                        content: Some("done".to_string()),
                        ..Default::default()
                    })
                }
            })
        }
    }
    let workspace = test_workspace()?;
    let seen: Seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut loop_ = Loop::with_session(
        Recorder {
            rounds: AtomicUsize::new(0),
            seen: seen.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?;
    loop_.steer("actually use tabs");
    let turn = loop_.turn("read the note").await?;
    assert_work_receipt(&turn, "done");
    let seen = seen.lock().map(|seen| seen.clone()).unwrap_or_default();
    assert!(
        seen.iter().any(|content| content == "actually use tabs"),
        "steering text reached the provider as a user message"
    );
    let replayed = loop_.replay_session();
    assert!(
        replayed
            .history
            .iter()
            .any(|message| message.content == "actually use tabs"),
        "steering text survived session replay"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn transient_provider_error_retries_with_note() -> Result<()> {
    struct Flaky {
        calls: AtomicUsize,
    }
    impl Provider for Flaky {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(Error::provider(
                        ProviderFailureKind::RateLimited,
                        "429 rate limit",
                    ))
                } else {
                    Ok(AssistantMessage {
                        content: Some("recovered".to_string()),
                        ..Default::default()
                    })
                }
            })
        }
    }
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        Flaky {
            calls: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "test",
    )?;
    let mut live = Vec::new();
    let turn = loop_
        .turn_with("hi", |event| live.push(event.clone()))
        .await?;
    assert_work_receipt(&turn, "recovered");
    assert!(
        live.iter().any(|event| matches!(
            event,
            Event::Note { text } if text.contains("retry 1/3")
        )),
        "the retry is visible: {live:?}"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn durable_events_commit_before_publish() -> Result<()> {
    let workspace = test_workspace()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut loop_ = schema_loop(&workspace, requests)?;
    let path = loop_.session_path().to_path_buf();
    let mut missing = Vec::new();

    loop_
        .turn_with("hi", |event| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read session: {error}"));
            let stored = match event {
                Event::Assistant { text: body } | Event::ToolResult { output: body, .. } => {
                    serde_json::to_string(body).is_ok_and(|body| text.contains(&body))
                }
                Event::ToolCall { name, .. } => text.contains(name),
                _ => true,
            };
            if !stored {
                missing.push(event.clone());
            }
        })
        .await?;

    assert!(missing.is_empty(), "published before commit: {missing:?}");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn permanent_error_does_not_retry() -> Result<()> {
    struct Dead {
        calls: std::sync::Arc<AtomicUsize>,
    }
    impl Provider for Dead {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Err(Error::Provider(ProviderFailure {
                    kind: ProviderFailureKind::Http,
                    status: Some(400),
                    message: "HTTP 400: request id 500 says connection closed".to_string(),
                    partial_output: false,
                }))
            })
        }
    }
    let workspace = test_workspace()?;
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let mut loop_ = Loop::with_session(
        Dead {
            calls: calls.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?;
    loop_.turn("hi").await.unwrap_err();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a permanent error fails without retrying"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn partial_stream_failure_does_not_retry() -> Result<()> {
    struct Partial(Arc<AtomicUsize>);
    impl Provider for Partial {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(Error::Provider(ProviderFailure {
                    kind: ProviderFailureKind::Transport,
                    status: None,
                    message: "connection reset after delta".to_string(),
                    partial_output: true,
                }))
            })
        }
    }
    let workspace = test_workspace()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut loop_ = Loop::with_session(Partial(calls.clone()), "test-model", &workspace, "test")?;
    loop_.turn("hi").await.unwrap_err();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn overflow_compacts_tool_results_and_retries() -> Result<()> {
    type Bodies = std::sync::Arc<std::sync::Mutex<Vec<String>>>;
    struct Swelling {
        calls: AtomicUsize,
        bodies: Bodies,
    }
    impl Provider for Swelling {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                let body = request
                    .messages
                    .iter()
                    .map(|message| message.content.clone())
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Ok(mut bodies) = self.bodies.lock() {
                    bodies.push(body);
                }
                match self.calls.fetch_add(1, Ordering::SeqCst) {
                    0 => Ok(AssistantMessage {
                        content: None,
                        tool_calls: vec![ToolCall {
                            id: "c".to_string(),
                            kind: ToolKind::Function,
                            function: ToolFunction {
                                name: "read_file".to_string(),
                                arguments: r#"{"path":"note.txt"}"#.to_string(),
                            },
                        }],
                        ..Default::default()
                    }),
                    1 => Err(Error::provider(
                        ProviderFailureKind::ContextOverflow,
                        "maximum context length exceeded for this model",
                    )),
                    _ => Ok(AssistantMessage {
                        content: Some("fits now".to_string()),
                        ..Default::default()
                    }),
                }
            })
        }
    }
    let workspace = test_workspace()?;
    let bodies: Bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut loop_ = Loop::with_session(
        Swelling {
            calls: AtomicUsize::new(0),
            bodies: bodies.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?;
    let turn = loop_.turn("read the note").await?;
    assert_work_receipt(&turn, "fits now");
    let bodies = bodies
        .lock()
        .map(|bodies| bodies.clone())
        .unwrap_or_default();
    assert!(
        bodies.last().is_some_and(|body| body.contains("[cleared")),
        "the retry went out compacted"
    );
    assert!(
        turn.events.iter().any(|event| matches!(
            event,
            Event::Note { text } if text.contains("overflow")
        )),
        "the overflow note is visible"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn long_tool_results_stay_until_window_compaction() -> Result<()> {
    type Bodies = std::sync::Arc<std::sync::Mutex<Vec<String>>>;
    struct Echo {
        bodies: Bodies,
    }
    impl Provider for Echo {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                let body = request
                    .messages
                    .iter()
                    .map(|message| message.content.clone())
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Ok(mut bodies) = self.bodies.lock() {
                    bodies.push(body);
                }
                Ok(AssistantMessage {
                    content: Some("ok".to_string()),
                    ..Default::default()
                })
            })
        }
    }
    let workspace = test_workspace()?;
    let bodies: Bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let bulky = "Y".repeat(90_000);
    let mut loop_ = Loop::with_session(
        Echo {
            bodies: bodies.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .restore(vec![
        Message::user("read it"),
        Message {
            role: crate::provider::Role::Assistant,
            content: String::new(),
            images: Vec::new(),
            tool_calls: vec![ToolCall {
                id: "c".to_string(),
                kind: ToolKind::Function,
                function: ToolFunction {
                    name: "read_file".to_string(),
                    arguments: r#"{"path":"note.txt"}"#.to_string(),
                },
            }],
            tool_call_id: None,
            reasoning: None,
        },
        Message::tool("c", bulky.clone()),
    ]);
    loop_.turn("continue").await?;
    let sent = bodies
        .lock()
        .map(|bodies| bodies.join("\n"))
        .unwrap_or_default();
    assert!(
        sent.contains(&bulky),
        "healthy rounds keep the original tool body for prefix cache"
    );
    assert!(
        !sent.contains("[cleared"),
        "in-place tool wipes are overflow-only"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn one_turn_advertises_one_tool_schema() -> Result<()> {
    type Lists = std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>;
    struct TwoRound {
        calls: AtomicUsize,
        lists: Lists,
    }
    impl Provider for TwoRound {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                let names = request
                    .tools
                    .iter()
                    .map(|tool| tool.function.name.clone())
                    .collect();
                if let Ok(mut lists) = self.lists.lock() {
                    lists.push(names);
                }
                match self.calls.fetch_add(1, Ordering::SeqCst) {
                    0 => Ok(AssistantMessage {
                        content: None,
                        tool_calls: vec![ToolCall {
                            id: "c".to_string(),
                            kind: ToolKind::Function,
                            function: ToolFunction {
                                name: "read_file".to_string(),
                                arguments: r#"{"path":"note.txt"}"#.to_string(),
                            },
                        }],
                        ..Default::default()
                    }),
                    _ => Ok(AssistantMessage {
                        content: Some("done".to_string()),
                        ..Default::default()
                    }),
                }
            })
        }
    }
    let workspace = test_workspace()?;
    let lists: Lists = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut loop_ = Loop::with_session(
        TwoRound {
            calls: AtomicUsize::new(0),
            lists: lists.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?;
    loop_.turn("read the note").await?;
    let lists = lists.lock().map(|lists| lists.clone()).unwrap_or_default();
    assert_eq!(lists.len(), 2, "tool then answer");
    assert_eq!(lists[0], lists[1], "both rounds ship the same tool schema");
    assert!(
        !lists[0].iter().any(|name| name == "agent"),
        "the retired nested runtime must not be advertised"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn auto_compact_summarizes_and_records() -> Result<()> {
    struct Summer;
    impl Provider for Summer {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                let asks_summary = request
                    .messages
                    .last()
                    .is_some_and(|message| message.content.contains("Summarize this session"));
                Ok(AssistantMessage {
                    content: Some(
                        if asks_summary {
                            "SUMMARY-CONTENT"
                        } else {
                            "done"
                        }
                        .to_string(),
                    ),
                    usage: Usage {
                        prompt_tokens: if asks_summary { 20 } else { 5 },
                        completion_tokens: if asks_summary { 4 } else { 1 },
                        reasoning_tokens: 0,
                        cached_tokens: 0,
                    },
                    ..Default::default()
                })
            })
        }
    }
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(Summer, "test-model", &workspace, "test")?;
    // a hair-trigger window: compact as soon as anything is said
    loop_.set_context_window(Some(16_386));
    let history = vec![
        Message::user("u1"),
        Message::user("u2"),
        Message::user("u3"),
        Message::user("u4"),
        Message::user("u5"),
        Message::user("u6"),
        Message::user("u7"),
    ];
    loop_ = loop_.restore(history);
    let mut live = Vec::new();
    let turn = loop_
        .turn_with("u8", |event| live.push(event.clone()))
        .await?;
    assert_work_receipt(&turn, "done");
    assert_eq!(turn.usage.prompt_tokens, 25);
    assert_eq!(turn.usage.completion_tokens, 5);
    assert_eq!(loop_.spent_tokens(), 30);
    let prefixes: Vec<&str> = live
        .iter()
        .filter_map(|event| match event {
            Event::Usage { prefix_hash, .. } => Some(prefix_hash.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(prefixes.len(), 2);
    assert!(!prefixes[0].is_empty());
    assert_eq!(
        prefixes[0], prefixes[1],
        "summary reused the live request head"
    );
    assert!(
        live.iter().any(|event| matches!(
            event,
            Event::Note { text } if text.contains("auto-compacted")
        )),
        "compaction is announced: {live:?}"
    );
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert!(log.contains("SUMMARY-CONTENT"), "summary recorded");
    let replayed = loop_.replay_session();
    assert!(
        replayed
            .history
            .iter()
            .any(|message| message.content.contains("SUMMARY-CONTENT")),
        "replay keeps the compact summary"
    );
    assert!(
        !replayed
            .history
            .iter()
            .any(|message| message.content == "u1"),
        "raw history is behind the checkpoint"
    );
    assert!(
        replayed
            .history
            .iter()
            .any(|message| message.content.starts_with("u8\n\nHarness state:")),
        "replay retains the active request"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn compaction_budget_stops_before_the_followup_request() -> Result<()> {
    struct Summer {
        calls: Arc<AtomicUsize>,
    }
    impl Provider for Summer {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(AssistantMessage {
                    content: Some("summary".to_string()),
                    usage: Usage {
                        prompt_tokens: 90,
                        completion_tokens: 10,
                        reasoning_tokens: 0,
                        cached_tokens: 0,
                    },
                    ..Default::default()
                })
            })
        }
    }
    let workspace = test_workspace()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut loop_ = Loop::with_session(
        Summer {
            calls: calls.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .restore((1..=7).map(|n| Message::user(format!("u{n}"))).collect())
    .with_budget(Budget {
        max_tokens: Some(100),
        max_secs: None,
    })?;
    loop_.set_context_window(Some(16_386));

    let turn = loop_.turn("u8").await?;

    assert!(turn.aborted);
    assert!(turn.text.contains("token limit"));
    assert_eq!(turn.usage.prompt_tokens, 90);
    assert_eq!(turn.usage.completion_tokens, 10);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[test]
fn provider_accepts_empty_key_as_local() {
    Client::new(" ", DEFAULT_BASE_URL).unwrap();
}
