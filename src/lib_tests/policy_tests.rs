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
    assert!(
        !request
            .messages
            .iter()
            .any(|message| message.content.contains("Do not expose this step yet"))
    );
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
