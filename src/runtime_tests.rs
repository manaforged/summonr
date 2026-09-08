use super::*;
use crate::endpoint::{Endpoint, Wire};
use crate::provider::Client;
use crate::workflow::{Role as WorkflowRole, Seat, Workflow};
use tempfile::tempdir;

#[tokio::test]
async fn workflow_activation_preserves_previous_roles_on_error() -> Result<()> {
    let workspace = tempdir()?;
    let endpoint = Endpoint {
        name: "fixture".to_string(),
        model: "review-model".to_string(),
        base_url: Some("http://127.0.0.1:1/v1".to_string()),
        api_key: None,
        source: None,
        oauth: None,
        body: None,
        context_window: None,
        idle_timeout_secs: None,
        effort: Some(Effort::High),
        price: None,
        env: Default::default(),
        headers: Default::default(),
        wire: Wire::Completions,
    };
    let seat = Seat {
        endpoint: "fixture".to_string(),
        model: None,
    };
    let mut loop_ = Loop::with_session(
        Client::new("", "http://127.0.0.1:1/v1")?,
        "driver",
        workspace.path(),
        "system",
    )?;
    let original = Workflow {
        name: "original".to_string(),
        seats: [
            (WorkflowRole::Review, seat.clone()),
            (WorkflowRole::Judge, seat.clone()),
        ]
        .into(),
    };
    loop_.apply_workflow(&original, |_| {
        Ok((Client::connect(&endpoint)?, endpoint.clone()))
    })?;
    let before = std::fs::read(loop_.session_path())?;
    let replacement = Workflow {
        name: "replacement".to_string(),
        seats: [
            (WorkflowRole::Plan, seat.clone()),
            (WorkflowRole::Review, seat),
        ]
        .into(),
    };
    let mut resolved = 0;
    let result = loop_.apply_workflow(&replacement, |_| {
        resolved += 1;
        if resolved == 2 {
            return Err(Error::Config("review endpoint unavailable".to_string()));
        }
        Ok((Client::connect(&endpoint)?, endpoint.clone()))
    });
    assert!(matches!(result, Err(Error::Config(message)) if message.contains("review seat")));
    assert_eq!(resolved, 2);
    assert_eq!(loop_.workflow().as_deref(), Some("original"));
    assert_eq!(loop_.seat_model(WorkflowRole::Review), Some("review-model"));
    assert_eq!(loop_.seat_model(WorkflowRole::Judge), Some("review-model"));
    assert_eq!(loop_.seat_model(WorkflowRole::Plan), None);
    assert_eq!(
        loop_.tuning.get(&WorkflowRole::Review),
        Some(&(Some(Effort::High), None))
    );
    assert_eq!(std::fs::read(loop_.session_path())?, before);

    let mut next = endpoint;
    next.model = "next-model".to_string();
    next.effort = None;
    loop_.apply_workflow(&replacement, |_| {
        Ok((Client::connect(&next)?, next.clone()))
    })?;
    assert_eq!(loop_.workflow().as_deref(), Some("replacement"));
    assert_eq!(loop_.seat_model(WorkflowRole::Review), Some("next-model"));
    assert_eq!(loop_.seat_model(WorkflowRole::Plan), Some("next-model"));
    assert_eq!(loop_.seat_model(WorkflowRole::Judge), None);
    assert!(!loop_.tuning.contains_key(&WorkflowRole::Judge));
    assert_eq!(loop_.tuning.get(&WorkflowRole::Review), Some(&(None, None)));
    Ok(())
}

impl<P: Provider> Loop<P> {
    pub(crate) async fn execute_tool(&self, call: &ToolCall) -> Result<String> {
        self.tools.execute(call).await
    }
}

#[tokio::test]
async fn observed_mail_is_paged_without_losing_replay_state() -> Result<()> {
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
            kind: "summoner".to_string(),
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

#[tokio::test]
async fn a_turn_resets_a_leaked_read_only_policy() -> Result<()> {
    #[derive(Clone)]
    struct Ack;
    impl Provider for Ack {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<AssistantMessage>> + Send + 'a>,
        > {
            Box::pin(async move {
                Ok(AssistantMessage {
                    content: Some("done".to_string()),
                    ..AssistantMessage::default()
                })
            })
        }
    }

    let workspace = std::env::temp_dir().join(format!(
        "mage-readonly-reset-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace)?;
    let mut loop_ = Loop::with_session(Ack, "test", &workspace, "system")?;
    loop_.tools.set_read_only(true);
    assert!(loop_.tools.is_read_only());
    loop_.turn_with("do the thing", |_event| {}).await?;
    assert!(
        !loop_.tools.is_read_only(),
        "a non-plan turn must clear a leaked read-only policy"
    );
    Ok(())
}

#[test]
fn the_default_schema_stays_under_budget_and_hides_idle_tools() -> Result<()> {
    let workspace = tempfile::tempdir()?;
    let tools = crate::tools::ToolRegistry::new(workspace.path())?;
    let mut definitions = tools.definitions();
    definitions.push(child::definition());
    let names: Vec<&str> = definitions
        .iter()
        .map(|tool| tool.function.name.as_str())
        .collect();
    let bytes = serde_json::to_string(&definitions)?.len();
    assert!(!names.contains(&"diagnostics"), "{names:?}");
    assert!(!names.contains(&"definition"), "{names:?}");
    assert_eq!(
        names.contains(&"skill"),
        crate::skills::catalog(workspace.path()).is_some()
    );
    assert!(bytes <= 4_600, "schema is {bytes} bytes: {names:?}");
    Ok(())
}
