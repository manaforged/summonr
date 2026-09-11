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
async fn usage_records_provider_time() -> Result<()> {
    struct Timed;
    impl Provider for Timed {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                Ok(AssistantMessage {
                    content: Some("done".to_string()),
                    usage: Usage {
                        prompt_tokens: 1,
                        completion_tokens: 1,
                        ..Usage::default()
                    },
                    ..AssistantMessage::default()
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(Timed, "test-model", &workspace, "system")?;
    let turn = loop_.turn("work").await?;
    assert!(
        turn.events.iter().any(
            |event| matches!(event, Event::Usage { millis: Some(millis), .. } if *millis >= 5)
        )
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn tool_schema_tracks_plan_mode() -> Result<()> {
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
    let names = requests
        .iter()
        .map(|request| {
            request
                .tools
                .iter()
                .map(|tool| tool.function.name.clone())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert!(names[0].iter().any(|name| name == "write_file"));
    assert!(!names[1].iter().any(|name| name == "write_file"));
    assert_eq!(names[0], names[2]);
    assert!(
        requests
            .iter()
            .all(|request| request.messages[0] == requests[0].messages[0])
    );
    let verification = requests
        .iter()
        .map(|request| {
            request
                .messages
                .iter()
                .rev()
                .find(|message| message.role == Role::User)
                .is_some_and(|message| message.content.contains("narrowest useful check"))
        })
        .collect::<Vec<_>>();
    assert_eq!(verification, vec![true, false, true]);
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

#[tokio::test]
async fn earlier_branch_excludes_later_turns_from_the_next_provider_request() -> Result<()> {
    let workspace = test_workspace()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut loop_ = schema_loop(&workspace, requests.clone())?;
    loop_.turn("keep this first turn").await?;
    let parent_path = loop_.session_path().to_path_buf();
    loop_.turn("exclude this later turn").await?;
    let parent = std::fs::read(&parent_path)?;
    let id = loop_.fork_at(Some(0))?;
    assert_eq!(loop_.session_id(), id);
    loop_.turn("continue from the branch").await?;
    let requests = captured(&requests)?;
    assert_eq!(requests.len(), 3);
    let messages = &requests[2].messages;
    assert!(
        messages
            .iter()
            .any(|message| message.content.contains("keep this first turn"))
    );
    assert!(
        messages
            .iter()
            .any(|message| message.content.contains("continue from the branch"))
    );
    assert!(
        !messages
            .iter()
            .any(|message| message.content.contains("exclude this later turn"))
    );
    assert_eq!(std::fs::read(parent_path)?, parent);
    assert_eq!(
        serde_json::to_vec(&requests[0].tools)?,
        serde_json::to_vec(&requests[2].tools)?
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[test]
fn harness_section_lists_plan_and_check() {
    let both = crate::prompt::harness_section(true, &["cargo test".to_string()]);
    assert!(both.contains("plan mode"));
    assert!(both.contains("cargo test"));
    assert!(both.contains("verified findings in severity order"));
    assert!(both.contains("A failed command proves nothing"));
    assert!(both.contains("Do not list an inference as a finding"));
    let automatic = crate::prompt::harness_section(false, &[]);
    assert!(automatic.contains("Auto mode"));
    assert!(automatic.contains("review"));
    assert!(automatic.contains("verified findings in severity order"));
    assert!(automatic.contains("Do not rank the codebase"));
    assert!(automatic.contains("without changing files or repository state"));
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
                                continuation: None,
                                id: "c1".to_string(),
                                kind: ToolKind::Function,
                                function: ToolFunction {
                                    name: "read_file".to_string(),
                                    arguments: r#"{"path":"note.txt"}"#.to_string(),
                                },
                            },
                            ToolCall {
                                continuation: None,
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
                            continuation: None,
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
            Event::Note { text } if text.contains("retrying in 2s")
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
                    retry_after: None,
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
async fn malformed_tool_call_gets_a_repair_prompt() -> Result<()> {
    struct Broken {
        calls: AtomicUsize,
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
    }

    impl Provider for Broken {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                self.requests
                    .lock()
                    .map_err(|_error| Error::Tool("request capture is poisoned".to_string()))?
                    .push(request.clone());
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(Error::Provider(ProviderFailure {
                        kind: ProviderFailureKind::MalformedToolCall,
                        status: Some(500),
                        message: "tool call arguments are invalid JSON".to_string(),
                        partial_output: false,
                        retry_after: None,
                    }));
                }
                Ok(AssistantMessage {
                    content: Some("recovered".to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut loop_ = Loop::with_session(
        Broken {
            calls: AtomicUsize::new(0),
            requests: requests.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?;
    let turn = loop_.turn("write the file").await?;
    assert_work_receipt(&turn, "recovered");
    assert!(turn.events.iter().any(|event| matches!(
        event,
        Event::Note { text } if text.contains("requesting a repaired call")
    )));
    let requests = captured(&requests)?;
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .messages
            .last()
            .is_some_and(|message| message.content.contains("invalid JSON"))
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
                    retry_after: None,
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
                            continuation: None,
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
                continuation: None,
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
                            continuation: None,
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
    loop_.set_context_window(Some(512));
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
async fn auto_compact_summarizes_one_long_autonomous_turn() -> Result<()> {
    struct LongTurn {
        rounds: AtomicUsize,
        summaries: Arc<AtomicUsize>,
    }
    impl Provider for LongTurn {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            let summary = request
                .messages
                .last()
                .is_some_and(|message| message.content.contains("Summarize this session"));
            if summary {
                self.summaries.fetch_add(1, Ordering::SeqCst);
                return Box::pin(async {
                    Ok(AssistantMessage {
                        content: Some("SUMMARY-CONTENT".to_string()),
                        ..Default::default()
                    })
                });
            }
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                assert!(
                    request.messages.iter().any(|message| {
                        message.role == Role::User
                            && message
                                .content
                                .contains("inspect the note repeatedly; preserve note.txt")
                    }),
                    "compaction lost the original objective and constraint"
                );
                if round < 12 {
                    Ok(AssistantMessage {
                        tool_calls: vec![ToolCall {
                            continuation: None,
                            id: format!("c{round}"),
                            kind: ToolKind::Function,
                            function: ToolFunction {
                                name: "read_file".to_string(),
                                arguments: r#"{"path":"note.txt"}"#.to_string(),
                            },
                        }],
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
    let summaries = Arc::new(AtomicUsize::new(0));
    let mut loop_ = Loop::with_session(
        LongTurn {
            rounds: AtomicUsize::new(0),
            summaries: summaries.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?;
    loop_.set_context_window(Some(256));
    let turn = loop_
        .turn("inspect the note repeatedly; preserve note.txt")
        .await?;
    assert_work_receipt(&turn, "done");
    assert!(
        summaries.load(Ordering::SeqCst) > 0,
        "summary was requested"
    );
    let replay = loop_.replay_session();
    assert!(
        replay
            .history
            .iter()
            .any(|message| message.content == "SUMMARY-CONTENT")
    );
    for (index, message) in replay.history.iter().enumerate() {
        if message.role != Role::Tool {
            continue;
        }
        let paired = replay.history[..index].iter().any(|assistant| {
            assistant.role == Role::Assistant
                && assistant
                    .tool_calls
                    .iter()
                    .any(|call| Some(call.id.as_str()) == message.tool_call_id.as_deref())
        });
        assert!(paired, "tool result retained without its assistant call");
    }
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn aside_rounds_stop_at_the_configured_token_budget() -> Result<()> {
    struct RepeatingAside {
        calls: Arc<AtomicUsize>,
    }
    impl Provider for RepeatingAside {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            if request.model != "review" {
                return Box::pin(async {
                    Ok(AssistantMessage {
                        content: Some("done".to_string()),
                        ..Default::default()
                    })
                });
            }
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(AssistantMessage {
                    tool_calls: vec![ToolCall {
                        continuation: None,
                        id: "inspect".to_string(),
                        kind: ToolKind::Function,
                        function: ToolFunction {
                            name: "read_file".to_string(),
                            arguments: r#"{"path":"note.txt"}"#.to_string(),
                        },
                    }],
                    usage: Usage {
                        prompt_tokens: 5,
                        completion_tokens: 5,
                        ..Default::default()
                    },
                    ..Default::default()
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = RepeatingAside {
        calls: calls.clone(),
    };
    let mut loop_ = Loop::with_session(
        RepeatingAside {
            calls: calls.clone(),
        },
        "test-model",
        &workspace,
        "test",
    )?
    .with_budget(Budget {
        max_tokens: Some(15),
        max_secs: None,
    })?;
    loop_.set_seat(
        crate::workflow::Role::Review,
        provider,
        "review",
        None,
        None,
    );
    let turn = loop_.turn("inspect the note").await?;
    assert!(
        turn.checks
            .iter()
            .any(|check| !check.passed && check.output.contains("token limit")),
        "{:?}",
        turn.checks
    );
    assert!(!loop_.work_progress()?.terminal());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn aside_provider_timeout_uses_the_configured_work_deadline() -> Result<()> {
    struct SlowAside;
    impl Provider for SlowAside {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                if request.model == "review" {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                Ok(AssistantMessage {
                    content: Some("late".to_string()),
                    ..Default::default()
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let mut loop_ =
        Loop::with_session(SlowAside, "test-model", &workspace, "test")?.with_budget(Budget {
            max_tokens: None,
            max_secs: Some(1),
        })?;
    loop_.set_seat(
        crate::workflow::Role::Review,
        SlowAside,
        "review",
        None,
        None,
    );
    let turn = loop_.turn("inspect the note").await?;
    assert!(turn.aborted, "{turn:?}");
    assert!(turn.text.contains("work-time limit"), "{turn:?}");
    assert!(loop_.elapsed_secs() >= 1);
    assert!(!loop_.work_progress()?.terminal());
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
    loop_.set_context_window(Some(512));

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

struct Tagged {
    tag: &'static str,
    calls: Arc<Mutex<Vec<(String, String)>>>,
}

impl Tagged {
    fn new(tag: &'static str, calls: &Arc<Mutex<Vec<(String, String)>>>) -> Self {
        Self {
            tag,
            calls: calls.clone(),
        }
    }
}

impl Provider for Tagged {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push((self.tag.to_string(), request.model.clone()));
            }
            let judge = request
                .messages
                .first()
                .is_some_and(|message| message.content.starts_with("You judge"));
            Ok(AssistantMessage {
                content: Some(if judge {
                    r#"{"done":true,"reason":"signed"}"#.to_string()
                } else {
                    format!("reply from {}", self.tag)
                }),
                ..AssistantMessage::default()
            })
        })
    }
}

#[tokio::test]
async fn roles_run_on_their_own_seats() -> Result<()> {
    use crate::workflow::Role as Step;

    let workspace = test_workspace()?;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut loop_ = Loop::with_session(
        Tagged::new("driver", &calls),
        "test-model",
        &workspace,
        "test",
    )?
    .with_verify(vec!["exit 0".to_string()]);
    loop_.set_seat(
        Step::Plan,
        Tagged::new("plan", &calls),
        "plan-model",
        None,
        None,
    );
    loop_.set_seat(
        Step::Judge,
        Tagged::new("judge", &calls),
        "judge-model",
        None,
        None,
    );
    assert_eq!(loop_.seat_model(Step::Plan), Some("plan-model"));
    assert_eq!(loop_.seat_model(Step::Implement), None);
    assert_eq!(loop_.workflow(), None);
    loop_.set_workflow(Some("cheap"))?;

    loop_.set_plan(true);
    let plan = loop_.turn("plan it").await?;
    assert_eq!(plan.text, "reply from plan");
    loop_.accept_plan()?;
    loop_.set_plan(false);
    let done = loop_.turn("do it").await?;
    assert!(done.text.starts_with("reply from driver"));
    loop_.set_seat(
        Step::Implement,
        Tagged::new("impl", &calls),
        "impl-model",
        None,
        None,
    );
    let gates = vec!["exit 0".to_string()];
    loop_.select_goal("finish", &gates, 3)?;
    let goal = loop_.turn(&crate::goal_prompt("finish", &gates, 0)).await?;
    assert!(goal.text.starts_with("reply from impl"));
    assert_eq!(loop_.work_progress()?.phase, crate::WorkPhase::Completed);

    let calls = calls
        .lock()
        .map_err(|_error| Error::Tool("call log is poisoned".to_string()))?
        .clone();
    assert_eq!(
        calls,
        [
            ("plan".to_string(), "plan-model".to_string()),
            ("driver".to_string(), "test-model".to_string()),
            ("impl".to_string(), "impl-model".to_string()),
            ("judge".to_string(), "judge-model".to_string()),
        ]
    );
    let log = std::fs::read_to_string(loop_.session_path())?;
    assert!(log.contains(r#""model":"judge-model""#), "{log}");
    assert!(log.contains(r#""kind":"workflow","name":"cheap""#), "{log}");
    assert_eq!(loop_.workflow().as_deref(), Some("cheap"));
    loop_.clear_seat(Step::Plan);
    assert_eq!(loop_.seat_model(Step::Plan), None);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn an_empty_response_still_charges_the_round() -> Result<()> {
    struct Empty;
    impl Provider for Empty {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                Ok(AssistantMessage {
                    content: Some(String::new()),
                    ..AssistantMessage::default()
                })
            })
        }
    }

    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(Empty, "test-model", &workspace, "test")?;
    let result = loop_.turn("hello").await;
    assert!(result.is_err(), "an empty response should fail the turn");
    assert!(
        loop_.replay_session().spent_tokens > 0,
        "the round the provider billed must reach the ledger"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn manual_compact_summarizes_everything_before_the_last_request() -> Result<()> {
    struct Summer;
    impl Provider for Summer {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                Ok(AssistantMessage {
                    content: Some("summary of the early turns".to_string()),
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
    let mut loop_ = Loop::with_session(Summer, "test-model", &workspace, "test")?;
    assert!(
        !loop_.compact().await?,
        "an empty session has nothing to compact"
    );
    for n in 1..=4 {
        drop(loop_.turn(&format!("u{n}")).await?);
    }
    let before = loop_.history_len();
    let spent = loop_.spent_tokens();
    assert!(loop_.compact().await?);
    let after = loop_.history_len();
    assert!(after < before, "{after} < {before}");
    assert_eq!(loop_.spent_tokens(), spent + 100, "the summary is charged");
    assert_eq!(
        loop_.replay_session().spent_tokens,
        loop_.spent_tokens(),
        "the charge is durable"
    );
    let replay = loop_.replay_session();
    assert!(
        replay
            .history
            .iter()
            .any(|message| message.content.contains("summary of the early turns")),
        "the summary stands in for the early turns"
    );
    assert!(
        replay
            .history
            .iter()
            .any(|message| message.content.starts_with("u4")),
        "the latest request survives"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn manual_compact_rejects_a_summary_that_calls_a_tool() -> Result<()> {
    struct Caller;
    impl Provider for Caller {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                assert!(request.tools.is_empty(), "compaction advertises no tools");
                Ok(AssistantMessage {
                    content: Some("let me look".to_string()),
                    tool_calls: vec![call("read_file", r#"{"path":"note.txt"}"#)],
                    ..Default::default()
                })
            })
        }
    }
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(Caller, "test-model", &workspace, "test")?;
    loop_ = loop_.restore((1..=4).map(|n| Message::user(format!("u{n}"))).collect());
    let before = loop_.history_len();
    let error = loop_
        .compact()
        .await
        .expect_err("tool call is not a summary");
    assert!(
        error.to_string().contains("tried to call a tool"),
        "{error}"
    );
    assert_eq!(loop_.history_len(), before, "the context is unchanged");
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn a_role_seat_keeps_its_own_effort_and_price() -> Result<()> {
    use crate::provider::{Effort, Price};
    use crate::workflow::Role;
    struct Quiet;
    impl Provider for Quiet {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                Ok(AssistantMessage {
                    content: Some(if request.model == "judge-model" {
                        r#"{"verdict":"done","reason":"complete"}"#.to_string()
                    } else {
                        "ok".to_string()
                    }),
                    usage: Usage {
                        prompt_tokens: 1_000_000,
                        completion_tokens: 0,
                        reasoning_tokens: 0,
                        cached_tokens: 0,
                    },
                    ..Default::default()
                })
            })
        }
    }
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(Quiet, "test-model", &workspace, "test")?;
    let driving = Price {
        input: 2.0,
        output: 0.0,
        cached: 0.0,
    };
    let judge = Price {
        input: 20.0,
        output: 0.0,
        cached: 0.0,
    };
    loop_.set_effort(Some(Effort::Low));
    loop_.set_price(Some(driving));
    loop_.set_seat(
        Role::Judge,
        Quiet,
        "judge-model",
        Some(Effort::High),
        Some(judge),
    );
    assert_eq!(loop_.effort_for(Role::Judge), Some(Effort::High));
    assert_eq!(loop_.effort_for(Role::Implement), Some(Effort::Low));
    assert_eq!(loop_.price_for(Role::Judge), Some(judge));
    assert_eq!(loop_.price_for(Role::Review), Some(driving));

    let turn = loop_.turn("hi").await?;
    let cost = turn.events.iter().find_map(|event| match event {
        Event::Usage { cost, .. } => *cost,
        _ => None,
    });
    assert_eq!(cost, Some(2.0), "the driving seat priced this round");

    loop_.select_goal("finish it", &[], 3)?;
    let turn = loop_.turn("go").await?;
    let judged = turn.events.iter().find_map(|event| match event {
        Event::Usage {
            prefix_hash, cost, ..
        } if prefix_hash == "judge" => *cost,
        _ => None,
    });
    assert_eq!(
        judged,
        Some(20.0),
        "the judge round is priced at its own seat"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn the_turn_after_a_plan_announces_the_accept() -> Result<()> {
    struct Planner;
    impl Provider for Planner {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                Ok(AssistantMessage {
                    content: Some("# Plan\n\n1. Do it".to_string()),
                    ..Default::default()
                })
            })
        }
    }
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(Planner, "test-model", &workspace, "test")?.with_plan();
    drop(loop_.turn("plan it").await?);
    loop_.set_plan(false);
    let turn = loop_.turn("now do it").await?;
    assert!(
        turn.events.iter().any(|event| matches!(
            event,
            Event::Note { text } if text.starts_with("plan accepted")
        )),
        "{:?}",
        turn.events
    );
    let replay = loop_.replay_session();
    assert!(
        replay.history.iter().any(|message| message
            .content
            .contains("the selected plan is now accepted")),
        "the model was told"
    );
    let again = loop_.turn("more").await?;
    assert!(
        !again.events.iter().any(
            |event| matches!(event, Event::Note { text } if text.starts_with("plan accepted"))
        ),
        "announced once"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn failed_auto_compaction_is_not_retried_every_round() -> Result<()> {
    struct Refusing {
        calls: AtomicUsize,
        summaries: AtomicUsize,
    }
    impl Provider for Refusing {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                let summary = request
                    .messages
                    .last()
                    .is_some_and(|message| message.content.contains("Summarize this session"));
                let read = || AssistantMessage {
                    content: None,
                    tool_calls: vec![ToolCall {
                        continuation: None,
                        id: "c".to_string(),
                        kind: ToolKind::Function,
                        function: ToolFunction {
                            name: "read_file".to_string(),
                            arguments: r#"{"path":"note.txt"}"#.to_string(),
                        },
                    }],
                    ..Default::default()
                };
                if summary {
                    self.summaries.fetch_add(1, Ordering::SeqCst);
                    return Ok(read());
                }
                match self.calls.fetch_add(1, Ordering::SeqCst) {
                    0..=2 => Ok(read()),
                    _ => Ok(AssistantMessage {
                        content: Some("done".to_string()),
                        ..Default::default()
                    }),
                }
            })
        }
    }
    let workspace = test_workspace()?;
    let provider = Refusing {
        calls: AtomicUsize::new(0),
        summaries: AtomicUsize::new(0),
    };
    let bulky = "Y".repeat(20_000);
    let mut history = Vec::new();
    for round in 0..4 {
        history.push(Message::user(format!("round {round}")));
        history.push(Message::tool("c", bulky.clone()));
    }
    let mut loop_ =
        Loop::with_session(provider, "test-model", &workspace, "test")?.restore(history);
    loop_.set_context_window(Some(2_000));

    let turn = loop_.turn("continue").await?;

    let failures = turn
        .events
        .iter()
        .filter(|event| matches!(event, Event::Note { text } if text.starts_with("auto-compaction failed")))
        .count();
    assert_eq!(failures, 1, "one failure note for the whole turn");
    assert!(
        turn.rounds >= 4,
        "the turn kept working after the failed summary"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[test]
fn module_switches_drop_their_tools_from_the_schema() -> Result<()> {
    struct Silent;
    impl Provider for Silent {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async {
                Ok(AssistantMessage {
                    content: Some("ok".to_string()),
                    ..Default::default()
                })
            })
        }

        fn child(&self) -> Option<Self> {
            Some(Self)
        }

        fn supports_children(&self) -> bool {
            true
        }
    }
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(Silent, "test-model", &workspace, "test")?;
    let before = loop_.tool_names();
    assert!(before.iter().any(|name| name == "task"));
    assert!(before.iter().any(|name| name == "run_command"));

    let user = std::collections::BTreeMap::from([
        ("lsp".to_string(), false),
        ("search".to_string(), false),
        ("skills".to_string(), false),
        ("task".to_string(), false),
    ]);
    let switches = crate::modules::Switches::from_maps(&user, &std::collections::BTreeMap::new());
    loop_.set_modules(&switches);

    let after = loop_.tool_names();
    assert!(
        after.iter().all(|name| !matches!(
            name.as_str(),
            "diagnostics" | "definition" | "skill" | "web_search" | "web_fetch" | "task"
        )),
        "{after:?}"
    );
    assert!(after.iter().any(|name| name == "run_command"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn a_context_probe_settles_before_a_later_round_without_blocking() -> Result<()> {
    let workspace = test_workspace()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut loop_ = schema_loop(&workspace, requests)?;
    assert_eq!(loop_.context_window(), None);
    loop_.set_context_probe(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        Some(4096)
    }));
    loop_.turn("hi").await?;
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    loop_.turn("again").await?;
    assert_eq!(loop_.context_window(), Some(4096));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}
