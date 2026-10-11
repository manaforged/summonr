use serde_json::json;

use super::*;
use crate::mcp::{Field, FieldKind, Form, FormAction, FormAnswer};
use crate::provider::Client;

fn session(workspace: &Path, home: &Path) -> Loop<Client> {
    let client = Client::new("k", "http://127.0.0.1:9/v1").unwrap();
    let driver = super::super::configure::open_driver(home, workspace, "m", "system").unwrap();
    Loop::open(client, "m", workspace, "system", driver).unwrap()
}

fn form() -> Form {
    Form {
        server: Some("fixture".to_string()),
        message: "Who?".to_string(),
        fields: vec![Field {
            name: "name".to_string(),
            title: "Name".to_string(),
            description: None,
            kind: FieldKind::Text {
                format: None,
                min_length: None,
                max_length: None,
                pattern: None,
            },
            required: true,
            default: None,
        }],
    }
}

async fn answered(reply: Option<&str>) -> (FormAnswer, Vec<Event>) {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut loop_ = session(workspace.path(), home.path());
    if let Some(reply) = reply {
        loop_.steer_with(Arc::new(Steering::answered([UserInput::from(
            reply.to_string(),
        )])));
    }
    let mut events = Vec::new();
    let answer = loop_
        .elicit(form(), &mut events, &mut |_| {})
        .await
        .unwrap();
    (answer, events)
}

#[tokio::test]
async fn a_user_answer_reaches_the_server_after_a_check() {
    let reply = json!({"action": "accept", "content": {"name": "Ada"}}).to_string();
    let (answer, events) = answered(Some(&reply)).await;
    assert_eq!(answer.action, FormAction::Accept);
    assert_eq!(answer.content.unwrap()["name"], json!("Ada"));
    assert!(events.iter().any(
        |event| matches!(event, Event::Elicit { form: Some(form) } if form.server.as_deref() == Some("fixture"))
    ));
    assert!(matches!(events.last(), Some(Event::Elicit { form: None })));
}

#[tokio::test]
async fn an_answer_that_breaks_the_form_is_not_sent() {
    let reply = json!({"action": "accept", "content": {}}).to_string();
    let (answer, _) = answered(Some(&reply)).await;
    assert_eq!(answer.action, FormAction::Cancel);
    assert!(answer.content.is_none());
}

#[tokio::test]
async fn a_form_without_a_user_is_declined() {
    let (answer, events) = answered(None).await;
    assert_eq!(answer.action, FormAction::Decline);
    assert!(events.is_empty());
}

#[tokio::test]
async fn a_pushed_answer_wakes_the_waiting_dialog() {
    let steering = Arc::new(Steering::default());
    let waiting = Arc::clone(&steering);
    let answer = tokio::spawn(async move {
        tokio::time::timeout(std::time::Duration::from_secs(5), waiting.next()).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    steering.answer(UserInput::from("go on"));
    let input = answer.await.unwrap().expect("the push wakes the waiter");
    assert_eq!(input.text, "go on");
}
