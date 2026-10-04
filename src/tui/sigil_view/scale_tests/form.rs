use serde_json::json;
use tokio::sync::mpsc::UnboundedReceiver;

use super::*;
use crate::mcp::{Choice, Field, FieldKind, Form, FormAction, FormAnswer};
use crate::tui::Cmd;
use crate::tui::sigil_view::app::Reply;

fn form() -> Form {
    Form {
        server: "fixture".to_owned(),
        message: "Who is asking?".to_owned(),
        fields: vec![
            Field {
                name: "name".to_owned(),
                title: "Name".to_owned(),
                description: None,
                kind: FieldKind::Text {
                    format: None,
                    min_length: None,
                    max_length: None,
                    pattern: None,
                },
                required: true,
                default: None,
            },
            Field {
                name: "age".to_owned(),
                title: "Age".to_owned(),
                description: None,
                kind: FieldKind::Number {
                    integer: true,
                    minimum: Some(0.0),
                    maximum: None,
                },
                required: false,
                default: None,
            },
            Field {
                name: "color".to_owned(),
                title: "Color".to_owned(),
                description: None,
                kind: FieldKind::Choice {
                    options: ["Red", "Green"]
                        .map(|value| Choice {
                            value: value.to_owned(),
                            title: value.to_owned(),
                        })
                        .to_vec(),
                },
                required: false,
                default: Some(json!("Red")),
            },
        ],
    }
}

fn open(workspace: &std::path::Path) -> (App, UnboundedReceiver<Cmd>) {
    std::fs::create_dir(workspace.join(".git")).unwrap();
    std::fs::write(workspace.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let (commands, receiver) = unbounded_channel();
    let app = App::new(busy_info(), workspace.to_owned(), commands);
    app.resize(100, 30);
    app.update(Msg::Event(Event::Elicit { form: Some(form()) }));
    (app, receiver)
}

fn typed(app: &App, text: &str) {
    app.replace(text.to_owned());
    assert!(app.form_enter());
}

fn sent(receiver: &mut UnboundedReceiver<Cmd>) -> FormAnswer {
    loop {
        match receiver.try_recv() {
            Ok(Cmd::Steer(input)) => return serde_json::from_str(&input.text).unwrap(),
            Ok(_other) => {}
            Err(error) => panic!("no answer was sent: {error}"),
        }
    }
}

#[test]
fn a_filled_form_is_checked_per_field_and_sent_after_review() {
    Runtime::scope("form-send", || {
        let workspace = tempfile::tempdir().unwrap();
        let (app, mut receiver) = open(workspace.path());
        typed(&app, "");
        assert!(
            app.form
                .peek()
                .unwrap()
                .lines()
                .iter()
                .any(|line| line.contains("Name is required"))
        );
        typed(&app, "Ada");
        typed(&app, "-3");
        assert!(
            app.form
                .peek()
                .unwrap()
                .lines()
                .iter()
                .any(|line| line.contains("out of range"))
        );
        typed(&app, "7");
        assert_eq!(app.draft.peek(), "Red");
        typed(&app, "2");
        let form = app.form.peek().unwrap();
        let review = form.lines().join("\n");
        assert!(review.contains("Color: Green"), "{review}");
        assert!(form.stage == crate::tui::sigil_view::app::Stage::Review);
        app.form_reply(Reply::Send);
        let answer = sent(&mut receiver);
        assert_eq!(answer.action, FormAction::Accept);
        assert_eq!(
            serde_json::Value::Object(answer.content.unwrap()),
            json!({"name": "Ada", "age": 7, "color": "Green"})
        );
        assert!(app.form.peek().is_none());
    });
}

#[test]
fn cancelling_the_form_sends_cancel() {
    Runtime::scope("form-cancel", || {
        let workspace = tempfile::tempdir().unwrap();
        let (app, mut receiver) = open(workspace.path());
        app.form_reply(Reply::Cancel);
        assert_eq!(sent(&mut receiver).action, FormAction::Cancel);
    });
}
