use super::*;
use serde_json::json;

#[tokio::test]
async fn write_glob_denies_other_paths() -> Result<()> {
    let workspace = test_workspace()?;
    let policy = crate::policy::Policy::builtin().with_write(&["src/**"]);
    let tools = crate::tools::ToolRegistry::with_policy(&workspace, policy)?;
    let denied = tools
        .execute(&call(
            "write_file",
            r#"{"path":"README.md","content":"no"}"#,
        ))
        .await;
    denied.unwrap_err();
    let escaped = tools
        .execute(&call(
            "write_file",
            r#"{"path":"src/../README.md","content":"escaped"}"#,
        ))
        .await;
    escaped.unwrap_err();
    assert!(!workspace.join("README.md").exists());
    let ok = tools
        .execute(&call(
            "write_file",
            r#"{"path":"src/ok.rs","content":"ok\n"}"#,
        ))
        .await?;
    assert!(ok.contains("wrote src/ok.rs"));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn embedded_registry_can_disable_web_tools() -> Result<()> {
    let workspace = test_workspace()?;
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    let names: Vec<_> = tools
        .definitions()
        .into_iter()
        .map(|tool| tool.function.name)
        .collect();
    assert!(!names.iter().any(|n| n == "web_search"));
    assert!(!names.iter().any(|n| n == "web_fetch"));
    let missing = tools
        .execute(&call("web_search", r#"{"query":"rust"}"#))
        .await;
    missing.unwrap_err();
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn native_web_search_hits_and_curl_is_allowed() -> Result<()> {
    let workspace = test_workspace()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || {
        let body = r#"<?xml version="1.0"?><rss><channel><item><title>Tokio</title><link>https://tokio.rs</link><description>async runtime</description></item></channel></rss>"#;
        let (mut stream, _) = listener.accept()?;
        let _ = read_http_request(&mut stream)?;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes())?;
        Ok::<_, std::io::Error>(())
    });
    let search = crate::web::Search::native_for_test(&format!("http://{address}/search"))?;
    let mut tools = crate::tools::ToolRegistry::with_policy_and_search(
        &workspace,
        crate::policy::Policy::builtin(),
        Some(search),
    )?;
    let names: Vec<_> = tools
        .definitions()
        .into_iter()
        .map(|tool| tool.function.name)
        .collect();
    assert!(names.iter().any(|n| n == "web_search"));
    assert!(names.iter().any(|n| n == "web_fetch"));
    let out = tools
        .execute(&call("web_search", r#"{"query":"tokio"}"#))
        .await?;
    assert!(out.contains("Tokio"));
    assert!(out.contains("https://tokio.rs"));
    let curl = tools
        .execute(&call(
            "run_command",
            &command("curl --version", "curl.exe --version"),
        ))
        .await?;
    assert!(curl.contains("exit 0"));
    tools.set_read_only(true);
    let plan: Vec<_> = tools
        .definitions()
        .into_iter()
        .map(|tool| tool.function.name)
        .collect();
    assert!(plan.iter().any(|n| n == "web_search"));
    assert!(!plan.iter().any(|n| n == "write_file"));
    assert!(!plan.iter().any(|n| n == "read_file"));
    tools
        .execute(&call("write_file", r#"{"path":"no","content":"no"}"#))
        .await
        .unwrap_err();
    server
        .join()
        .map_err(|_panic| Error::Tool("search fixture panicked".to_string()))?
        .map_err(|error| Error::Tool(error.to_string()))?;
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn web_fetch_rejects_ssrf() -> Result<()> {
    let workspace = test_workspace()?;
    let search = crate::web::Search::native_for_test("http://127.0.0.1:9/search")?;
    let tools = crate::tools::ToolRegistry::with_policy_and_search(
        &workspace,
        crate::policy::Policy::builtin(),
        Some(search),
    )?;
    let denied = tools
        .execute(&call("web_fetch", r#"{"url":"http://127.0.0.1/"}"#))
        .await;
    denied.unwrap_err();
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn external_steps_are_owned_by_work() -> Result<()> {
    struct Capture(Arc<Mutex<Option<CompletionRequest>>>);
    impl Provider for Capture {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
            Box::pin(async move {
                *self
                    .0
                    .lock()
                    .map_err(|_error| Error::Tool("request capture is poisoned".to_string()))? =
                    Some(request.clone());
                Ok(AssistantMessage {
                    content: Some("done".to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }

    let workspace = test_workspace()?;

    let captured = Arc::new(Mutex::new(None));
    let mut loop_ = Loop::with_session(
        Capture(captured.clone()),
        "test-model",
        &workspace,
        "system",
    )?;
    loop_.select_steps(
        "## Steps\n\n1. Implement it\n2. Do not expose this step yet\n",
        &[],
    )?;
    loop_.turn("implement it").await?;
    let request = captured
        .lock()
        .map_err(|_error| Error::Tool("request capture is poisoned".to_string()))?
        .clone()
        .ok_or_else(|| Error::Tool("provider request was not captured".to_string()))?;
    assert!(request.messages.last().is_some_and(|message| {
        message
            .content
            .contains("Steps or Goal runner owns this Work")
    }));
    assert!(request.messages.last().is_some_and(|message| {
        message.content.contains("Harness ledger")
            && message.content.contains("[/] Implement it")
            && message.content.contains("[ ] Do not expose this step yet")
    }));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

struct CommandProvider {
    rounds: AtomicUsize,
}

impl Provider for CommandProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            Ok(match round {
                0 => AssistantMessage {
                    tool_calls: vec![call(
                        "run_command",
                        &command("printf guarded", "[Console]::Write('guarded')"),
                    )],
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
async fn commands_execute_inside_work() -> Result<()> {
    let workspace = test_workspace()?;

    let mut loop_ = Loop::with_session(
        CommandProvider {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "system",
    )?;

    let turn = loop_.turn("run it").await?;

    assert_work_receipt(&turn, "done");
    assert_eq!(turn.rounds, 2);
    assert!(turn.events.iter().any(|event| matches!(
        event,
        Event::ToolResult { success: true, output, .. } if output.contains("guarded")
    )));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

struct JobProvider {
    rounds: AtomicUsize,
}

impl Provider for JobProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            Ok(match round {
                0 => AssistantMessage {
                    tool_calls: vec![call(
                        "run_command",
                        &json!({"command": if cfg!(windows) {
                            "Start-Sleep -Milliseconds 300; [IO.File]::WriteAllText('done.txt', \"built`n\")"
                        } else { "sleep 0.3; echo built > done.txt" }, "background": true}).to_string(),
                    )],
                    ..AssistantMessage::default()
                },
                _ => AssistantMessage {
                    content: Some("started the build in the background".to_string()),
                    ..AssistantMessage::default()
                },
            })
        })
    }
}

#[tokio::test]
async fn a_turn_waits_for_its_background_job_before_verify() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        JobProvider {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "system",
    )?;

    let turn = loop_.turn("build it").await?;

    assert_eq!(
        std::fs::read_to_string(workspace.join("done.txt"))?.trim(),
        "built",
        "the job finished before the turn closed"
    );
    assert!(turn.events.iter().any(|event| matches!(
        event,
        Event::Verify { command, passed: true } if command.starts_with("job ")
    )));
    let receipt = turn_receipt(&turn);
    assert!(
        receipt.text.contains("Changed: `done.txt`"),
        "{}",
        receipt.text
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

struct TwoReads {
    rounds: AtomicUsize,
}

impl Provider for TwoReads {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            Ok(match round {
                0 => AssistantMessage {
                    tool_calls: vec![
                        ToolCall {
                            continuation: None,
                            id: "first".to_string(),
                            ..call("grep", r#"{"pattern":"hello"}"#)
                        },
                        ToolCall {
                            continuation: None,
                            id: "second".to_string(),
                            ..call("read_file", r#"{"path":"note.txt"}"#)
                        },
                    ],
                    ..AssistantMessage::default()
                },
                _ => AssistantMessage {
                    content: Some("read both".to_string()),
                    ..AssistantMessage::default()
                },
            })
        })
    }
}

#[tokio::test]
async fn read_only_calls_in_one_round_answer_in_call_order() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        TwoReads {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "system",
    )?;
    let turn = loop_.turn("look").await?;
    let results: Vec<(&str, bool)> = turn
        .events
        .iter()
        .filter_map(|event| match event {
            Event::ToolResult { id, success, .. } => Some((id.as_str(), *success)),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(results.iter().all(|(_, ok)| *ok), "{results:?}");
    let calls: Vec<&str> = turn
        .events
        .iter()
        .filter_map(|event| match event {
            Event::ToolCall { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, ["first", "second"]);
    assert_eq!(
        results.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        calls,
        "results keep the call order"
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

struct FailingJob {
    rounds: AtomicUsize,
}

impl Provider for FailingJob {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            Ok(match round {
                0 => AssistantMessage {
                    tool_calls: vec![call(
                        "run_command",
                        r#"{"command":"exit 3","background":true}"#,
                    )],
                    ..AssistantMessage::default()
                },
                _ => AssistantMessage {
                    content: Some("kicked off the build".to_string()),
                    ..AssistantMessage::default()
                },
            })
        })
    }
}

#[tokio::test]
async fn a_background_job_that_already_failed_fails_the_turn() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        FailingJob {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "system",
    )?
    .with_verify(vec!["exit 0".to_string()]);
    let turn = loop_.turn("build it").await?;
    assert!(
        turn.checks.iter().any(|check| !check.passed),
        "the failed job is the turn's verdict even though the declared check passed"
    );
    assert!(turn.events.iter().any(|event| matches!(
        event,
        Event::Verify { command, passed: false } if command.starts_with("job ")
    )));
    let receipt = turn_receipt(&turn);
    assert!(receipt.text.contains("failed"), "{}", receipt.text);
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

struct Retry {
    rounds: AtomicUsize,
    workspace: PathBuf,
}

impl Provider for Retry {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            Ok(match round {
                0 | 2 => AssistantMessage {
                    tool_calls: vec![call(
                        "run_command",
                        &json!({
                            "command": if cfg!(windows) {
                                "if (Test-Path -LiteralPath ok.txt) { exit 0 } else { exit 1 }"
                            } else {
                                "test -f ok.txt"
                            },
                            "background": true,
                        })
                        .to_string(),
                    )],
                    ..AssistantMessage::default()
                },
                1 => {
                    let record = crate::jobs::since(&self.workspace, 0, None)
                        .pop()
                        .expect("first job");
                    tokio::time::timeout(std::time::Duration::from_secs(10), async {
                        loop {
                            let status = crate::jobs::status(&self.workspace, &record.id)?;
                            if !status.running {
                                assert_eq!(
                                    status.exit_code,
                                    Some(1),
                                    "first attempt must fail before the repair"
                                );
                                break Ok::<(), Error>(());
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        }
                    })
                    .await
                    .expect("first job exceeded ten seconds")?;
                    AssistantMessage {
                        tool_calls: vec![call(
                            "write_file",
                            r#"{"path":"ok.txt","content":"fixed\n"}"#,
                        )],
                        ..AssistantMessage::default()
                    }
                }
                _ => AssistantMessage {
                    content: Some("fixed and rebuilt".to_string()),
                    ..AssistantMessage::default()
                },
            })
        })
    }
}

#[tokio::test]
async fn a_rerun_of_the_same_command_supersedes_its_failed_first_try() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        Retry {
            rounds: AtomicUsize::new(0),
            workspace: workspace.clone(),
        },
        "test-model",
        &workspace,
        "system",
    )?;
    let turn = loop_.turn("build it").await?;
    assert_eq!(turn.checks.len(), 1, "only the latest attempt should count");
    assert!(
        !turn
            .events
            .iter()
            .any(|event| matches!(event, Event::Verify { passed: false, .. })),
        "{:?}",
        turn.events
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

struct LongJob {
    rounds: AtomicUsize,
}

impl Provider for LongJob {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            Ok(match round {
                0 => AssistantMessage {
                    tool_calls: vec![call(
                        "run_command",
                        r#"{"command":"sleep 20","background":true}"#,
                    )],
                    ..AssistantMessage::default()
                },
                _ => AssistantMessage {
                    content: Some("all done".to_string()),
                    ..AssistantMessage::default()
                },
            })
        })
    }
}

#[tokio::test]
async fn a_turn_with_a_job_still_running_is_deferred_not_judged() -> Result<()> {
    let workspace = test_workspace()?;
    let mut loop_ = Loop::with_session(
        LongJob {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "system",
    )?;
    let turn = loop_.turn("build it").await?;
    assert!(turn.checks.is_empty(), "no verdict was taken");
    assert!(
        turn.events.iter().any(|event| matches!(
            event,
            Event::Note { text } if text.starts_with("turn deferred")
        )),
        "{:?}",
        turn.events
    );
    let progress = loop_.work_progress()?;
    assert_eq!(
        progress.phase,
        crate::WorkPhase::Execute,
        "the Work stays open"
    );
    assert_eq!(progress.attempts, 0, "no repair strike");
    let again = loop_.turn("is it done?").await?;
    assert!(
        again.checks.is_empty(),
        "the job from the first turn still gates"
    );
    assert!(
        again.events.iter().any(|event| matches!(
            event,
            Event::Note { text } if text.starts_with("turn deferred")
        )),
        "{:?}",
        again.events
    );
    assert_eq!(loop_.work_progress()?.phase, crate::WorkPhase::Execute);
    let tools = crate::tools::ToolRegistry::new(&workspace)?;
    for record in crate::jobs::since(&workspace, 0, None) {
        tools
            .execute(&call(
                "run_command",
                &json!({"job": record.id, "kill": true}).to_string(),
            ))
            .await?;
    }
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

fn answer_later(queue: Arc<Mutex<std::collections::VecDeque<UserInput>>>, text: &str) {
    let text = text.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if let Ok(mut queue) = queue.lock() {
            queue.push_back(UserInput::from(text));
        }
    });
}

struct WriteOutside {
    rounds: AtomicUsize,
    path: PathBuf,
}

impl Provider for WriteOutside {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            Ok(match round {
                0 => AssistantMessage {
                    tool_calls: vec![call(
                        "write_file",
                        &serde_json::json!({"path": self.path, "content": "hello\n"}).to_string(),
                    )],
                    ..AssistantMessage::default()
                },
                _ => AssistantMessage {
                    content: Some("wrote outside".to_string()),
                    ..AssistantMessage::default()
                },
            })
        })
    }
}

#[tokio::test]
async fn a_write_denial_retries_after_allow() -> Result<()> {
    let workspace = test_workspace()?;
    let outside = std::env::temp_dir().join(format!(
        "mage-grant-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&outside)?;
    let target = outside.join("note.txt");
    let mut loop_ = Loop::with_session(
        WriteOutside {
            rounds: AtomicUsize::new(0),
            path: target.clone(),
        },
        "test-model",
        &workspace,
        "system",
    )?;
    let queue = Arc::new(Mutex::new(std::collections::VecDeque::new()));
    loop_.steer_with(queue.clone());
    answer_later(queue, "y");
    let turn = loop_.turn("write it").await?;
    assert_eq!(std::fs::read_to_string(&target)?, "hello\n");
    assert!(
        turn.events.iter().any(|event| matches!(
            event,
            Event::Approval {
                detail: Some(_),
                ..
            }
        )),
        "{:?}",
        turn.events
    );
    assert!(
        turn.events.iter().any(|event| matches!(
            event,
            Event::ToolResult { success: true, output, .. } if output.contains("wrote")
        )),
        "{:?}",
        turn.events
    );
    std::fs::remove_dir_all(&outside)?;
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn a_write_denial_stays_denied_after_deny() -> Result<()> {
    let workspace = test_workspace()?;
    let outside = std::env::temp_dir().join(format!(
        "mage-deny-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&outside)?;
    let target = outside.join("note.txt");
    let mut loop_ = Loop::with_session(
        WriteOutside {
            rounds: AtomicUsize::new(0),
            path: target.clone(),
        },
        "test-model",
        &workspace,
        "system",
    )?;
    let queue = Arc::new(Mutex::new(std::collections::VecDeque::new()));
    loop_.steer_with(queue.clone());
    answer_later(queue, "n");
    let turn = loop_.turn("write it").await?;
    assert!(!target.exists(), "deny must not create the file");
    assert!(
        turn.events.iter().any(|event| matches!(
            event,
            Event::ToolResult { success: false, output, .. } if output.contains("/write")
        )),
        "{:?}",
        turn.events
    );
    std::fs::remove_dir_all(&outside)?;
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}
