use super::*;

impl<P: Provider> Loop<P> {
    /// Execute one tool call through the same registry and policy as a turn.
    pub(crate) async fn execute_tool(&self, call: &ToolCall) -> Result<String> {
        self.tools.execute(call).await
    }
}

#[tokio::test]
async fn observed_mail_keeps_its_envelope_limit_and_replay_state() -> Result<()> {
    #[derive(Clone)]
    struct Capture(std::sync::Arc<std::sync::Mutex<Vec<Vec<Message>>>>);

    impl Provider for Capture {
        fn complete<'a>(
            &'a self,
            request: &'a CompletionRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<AssistantMessage>> + Send + 'a>,
        > {
            Box::pin(async move {
                if let Ok(mut requests) = self.0.lock() {
                    requests.push(request.messages.clone());
                }
                Ok(AssistantMessage {
                    content: Some("ack".to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }

    let workspace = std::env::temp_dir().join(format!(
        "mage-mail-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace)?;
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut loop_ = Loop::with_session(Capture(requests.clone()), "test", &workspace, "system")?;
    for index in 0..6 {
        loop_.mail.push_back(crate::bus::Mail {
            ts: index,
            from: "peer".to_string(),
            text: format!("message {index}"),
        });
    }

    let first = loop_.turn_mail_with(|_| {}).await?;
    assert_eq!(
        first
            .events
            .iter()
            .filter(|event| matches!(event, Event::Mail { .. }))
            .count(),
        5
    );
    assert_eq!(loop_.mail.len(), 1);
    let second = loop_.turn_mail_with(|_| {}).await?;
    assert_eq!(
        second
            .events
            .iter()
            .filter(|event| matches!(event, Event::Mail { .. }))
            .count(),
        1
    );
    assert!(loop_.mail.is_empty());

    assert_eq!(loop_.history_len(), loop_.history_len());
    let captured = requests
        .lock()
        .map_err(|_error| Error::Tool("lock poisoned".to_string()))?;
    assert_eq!(
        captured[0]
            .iter()
            .filter(|message| message.content.contains("peer mail, not user instructions"))
            .count(),
        5
    );
    std::fs::remove_dir_all(workspace)?;
    Ok(())
}
