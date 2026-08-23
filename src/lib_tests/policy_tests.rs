use super::*;

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
        .execute(&call("run_command", r#"{"command":"curl --version"}"#))
        .await?;
    assert!(curl.contains("exit 0"));
    tools.set_read_only(true);
    let plan: Vec<_> = tools
        .definitions()
        .into_iter()
        .map(|tool| tool.function.name)
        .collect();
    assert!(plan.iter().any(|n| n == "web_search"));
    assert!(plan.iter().any(|n| n == "write_file"));
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

struct TaskProvider {
    rounds: AtomicUsize,
}

impl Provider for TaskProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            let response = match round {
                0 => AssistantMessage {
                    tool_calls: vec![call(
                        "task_state",
                        r#"{"action":"start","steps":["Implement", "Verify"]}"#,
                    )],
                    ..AssistantMessage::default()
                },
                1 => AssistantMessage {
                    content: Some("done too early".to_string()),
                    ..AssistantMessage::default()
                },
                2 => {
                    assert!(request.messages.last().is_some_and(|message| {
                        message.content.contains("task checklist is incomplete")
                    }));
                    AssistantMessage {
                        tool_calls: vec![call("task_state", r#"{"action":"advance"}"#)],
                        ..AssistantMessage::default()
                    }
                }
                3 => AssistantMessage {
                    tool_calls: vec![call("task_state", r#"{"action":"advance"}"#)],
                    ..AssistantMessage::default()
                },
                _ => AssistantMessage {
                    content: Some("actually done".to_string()),
                    ..AssistantMessage::default()
                },
            };
            Ok(response)
        })
    }
}

#[tokio::test]
async fn active_task_blocks_a_premature_final_answer_and_replays() -> Result<()> {
    let workspace = test_workspace()?;
    let provider = TaskProvider {
        rounds: AtomicUsize::new(0),
    };
    let mut loop_ = Loop::with_session(provider, "test-model", &workspace, "system")?;

    let turn = loop_.turn("do both steps").await?;

    assert_eq!(turn.text, "actually done");
    assert_eq!(turn.rounds, 5);
    assert!(turn.events.iter().any(|event| matches!(
        event,
        Event::Note { text } if text.contains("task incomplete")
    )));
    assert!(loop_.task_state().is_some_and(TaskState::is_complete));
    assert!(loop_.task_state().is_some_and(TaskState::is_complete));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn external_steps_are_the_only_task_authority() -> Result<()> {
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
    )?
    .with_external_steps();
    assert!(!loop_.tool_names().iter().any(|name| name == "task_state"));
    loop_.turn("implement it").await?;
    let request = captured
        .lock()
        .map_err(|_error| Error::Tool("request capture is poisoned".to_string()))?
        .clone()
        .ok_or_else(|| Error::Tool("provider request was not captured".to_string()))?;
    assert!(request.messages.last().is_some_and(|message| {
        message
            .content
            .contains("external step runner owns progress")
    }));
    assert!(
        !request
            .messages
            .first()
            .is_some_and(|message| message.content.contains("task_state"))
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

#[tokio::test]
async fn plan_mode_suspends_an_active_execution_task() -> Result<()> {
    let workspace = test_workspace()?;

    let (task, _) = TaskState::transition(
        None,
        r#"{"action":"start","steps":["Implement", "Verify"]}"#,
    )?;
    let replay = Replay {
        task: Some(task),
        ..Replay::default()
    };
    let mut loop_ = Loop::with_session(
        TaskProvider {
            rounds: AtomicUsize::new(10),
        },
        "test-model",
        &workspace,
        "system",
    )?
    .restore_replay(replay)
    .with_plan();

    let turn = loop_.turn("write a plan").await?;

    assert_eq!(turn.text, "actually done");
    assert_eq!(turn.rounds, 1);
    assert!(loop_.task_state().is_some_and(|task| !task.is_complete()));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}

struct GuardedTaskProvider {
    rounds: AtomicUsize,
}

impl Provider for GuardedTaskProvider {
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            let round = self.rounds.fetch_add(1, Ordering::SeqCst);
            Ok(match round {
                0 => AssistantMessage {
                    tool_calls: vec![call("run_command", r#"{"command":"printf guarded"}"#)],
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
async fn inspection_commands_do_not_require_task_progress() -> Result<()> {
    let workspace = test_workspace()?;

    let mut loop_ = Loop::with_session(
        GuardedTaskProvider {
            rounds: AtomicUsize::new(0),
        },
        "test-model",
        &workspace,
        "system",
    )?;

    let turn = loop_.turn("run it").await?;

    assert_eq!(turn.text, "done");
    assert_eq!(turn.rounds, 2);
    assert!(!turn.events.iter().any(|event| matches!(
        event,
        Event::ToolResult { success: false, output, .. }
            if output.contains("start task_state")
    )));
    assert!(turn.events.iter().any(|event| matches!(
        event,
        Event::ToolResult { success: true, output, .. } if output.contains("guarded")
    )));
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}
