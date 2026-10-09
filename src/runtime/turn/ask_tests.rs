use serde_json::json;

use super::*;
use crate::provider::{Client, ToolFunction, ToolKind};

fn session(workspace: &Path, home: &Path) -> Loop<Client> {
    let client = Client::new("k", "http://127.0.0.1:9/v1").unwrap();
    let driver =
        super::super::super::configure::open_driver(home, workspace, "m", "system").unwrap();
    Loop::open(client, "m", workspace, "system", driver).unwrap()
}

fn call(arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: "a1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: ASK.to_string(),
            arguments: arguments.to_string(),
        },
        continuation: None,
    }
}

fn offered(loop_: &Loop<Client>) -> bool {
    loop_
        .tool_definitions()
        .iter()
        .any(|definition| definition.function.name == ASK)
}

#[tokio::test]
async fn model_question_answers_reach_the_model_and_a_dismissal_says_so() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut loop_ = session(workspace.path(), home.path());
    let answers = VecDeque::from([
        UserInput::from(
            json!({"action": "accept", "content": {"1": "Postgres", "2": ["Linux", "my own"]}})
                .to_string(),
        ),
        UserInput::from(json!({"action": "cancel"}).to_string()),
    ]);
    loop_.steer_with(Arc::new(Steering::from(answers)));
    let questions = json!({"questions": [
        {"question": "Which database?", "options": [{"label": "Postgres", "description": "server"}, {"label": "SQLite"}]},
        {"question": "Targets?", "options": [{"label": "Linux"}, {"label": "macOS"}], "multiple": true},
        {"question": "Anything else?"}
    ]});
    let mut events = Vec::new();
    let output = loop_
        .ask(&call(questions.clone()), &mut events, &mut |_| {})
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        output.text,
        "Which database? → Postgres\nTargets? → Linux, my own\nAnything else? → (no answer)"
    );
    let Some(Event::Elicit { form: Some(form) }) = events.first() else {
        panic!("no question was shown: {events:?}");
    };
    assert_eq!(form.server, None);
    assert!(matches!(
        &form.fields[0].kind,
        FieldKind::Choice { options, custom: true } if options[0].description.as_deref() == Some("server")
    ));
    let dismissed = loop_
        .ask(&call(questions), &mut Vec::new(), &mut |_| {})
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        dismissed.text,
        "The user dismissed the question without answering."
    );
}

#[test]
fn model_question_is_offered_only_when_a_person_can_answer() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut loop_ = session(workspace.path(), home.path());
    assert!(!offered(&loop_));
    loop_.steer_with(Arc::new(Steering::default()));
    assert!(offered(&loop_));
    loop_.depth = 1;
    assert!(!offered(&loop_));
}
