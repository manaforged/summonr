use serde_json::json;
use tokio::sync::mpsc::UnboundedReceiver;

use super::*;
use crate::mcp::{Choice, Field, FieldKind, Form, FormAction, FormAnswer};
use crate::tui::Cmd;
use crate::tui::sigil_view::app::Reply;

fn form() -> Form {
    Form {
        server: Some("fixture".to_owned()),
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
                    options: choices(&["Red", "Green"]),
                    custom: false,
                },
                required: false,
                default: Some(json!("Red")),
            },
        ],
    }
}

fn choices(values: &[&str]) -> Vec<Choice> {
    values
        .iter()
        .map(|value| Choice {
            value: (*value).to_owned(),
            title: (*value).to_owned(),
            description: None,
        })
        .collect()
}

fn question(title: &str, kind: FieldKind) -> Field {
    Field {
        name: title.to_owned(),
        title: title.to_owned(),
        description: None,
        kind,
        required: false,
        default: None,
    }
}

fn open(workspace: &std::path::Path) -> (App, UnboundedReceiver<Cmd>) {
    opened(workspace, form())
}

fn opened(workspace: &std::path::Path, form: Form) -> (App, UnboundedReceiver<Cmd>) {
    std::fs::create_dir(workspace.join(".git")).unwrap();
    std::fs::write(workspace.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let (commands, receiver) = unbounded_channel();
    let app = App::new(busy_info(), workspace.to_owned(), commands);
    app.resize(100, 30);
    app.update(Msg::Event(Event::Elicit {
        form: Some(form),
        dialog: 1,
    }));
    (app, receiver)
}

fn typed(app: &App, text: &str) {
    app.replace(text.to_owned());
    assert!(app.form_enter());
}

fn sent(receiver: &mut UnboundedReceiver<Cmd>) -> FormAnswer {
    loop {
        match receiver.try_recv() {
            Ok(Cmd::Answer(_, text)) => return serde_json::from_str(&text).unwrap(),
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
                .shown("")
                .iter()
                .any(|line| line.contains("Name is required"))
        );
        typed(&app, "Ada");
        typed(&app, "-3");
        assert!(
            app.form
                .peek()
                .unwrap()
                .shown("")
                .iter()
                .any(|line| line.contains("out of range"))
        );
        typed(&app, "7");
        assert_eq!(app.draft.peek(), "");
        let lines = app.form.peek().unwrap().shown("");
        assert!(lines.iter().any(|line| line == "❯ 1. Red"), "{lines:?}");
        typed(&app, "2");
        let form = app.form.peek().unwrap();
        let review = form.shown("").join("\n");
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
fn enter_on_the_review_sends_the_answers() {
    Runtime::scope("form-review-enter", || {
        let workspace = tempfile::tempdir().unwrap();
        let (app, mut receiver) = open(workspace.path());
        typed(&app, "Ada");
        typed(&app, "7");
        typed(&app, "");
        assert_eq!(
            app.form_stage(),
            Some(crate::tui::sigil_view::app::Stage::Review)
        );
        assert!(app.form_enter());
        let answer = sent(&mut receiver);
        assert_eq!(answer.action, FormAction::Accept);
        assert_eq!(
            serde_json::Value::Object(answer.content.unwrap()),
            json!({"name": "Ada", "age": 7, "color": "Red"})
        );
    });
}

#[test]
fn a_question_shows_one_key_line_and_no_bare_cancel() {
    let form = Form {
        server: None,
        message: "Which database?".to_owned(),
        fields: vec![question(
            "Which database?",
            FieldKind::Choice {
                options: choices(&["Postgres", "SQLite"]),
                custom: true,
            },
        )],
    };
    let lines = frame_with(busy_info(), 100, 30, State::Idle, move |app| {
        app.update(Msg::Event(Event::Elicit {
            form: Some(form),
            dialog: 1,
        }));
    });
    let shown = lines.join("\n");
    assert!(shown.contains("2. SQLite"), "{shown}");
    assert_eq!(shown.matches("esc cancel").count(), 1, "{shown}");
    assert!(!shown.contains("↵ next"), "{shown}");
    assert!(!lines.iter().any(|line| line.trim() == "Cancel"), "{shown}");
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

#[test]
fn model_question_picks_with_arrows_and_sends_without_review() {
    Runtime::scope("form-model-pick", || {
        let workspace = tempfile::tempdir().unwrap();
        let form = Form {
            server: None,
            message: "Which database?".to_owned(),
            fields: vec![question(
                "Which database?",
                FieldKind::Choice {
                    options: choices(&["Postgres", "SQLite", "MySQL"]),
                    custom: true,
                },
            )],
        };
        let (app, mut receiver) = opened(workspace.path(), form);
        let lines = app.form.peek().unwrap().shown("");
        assert_eq!(lines[0], "The model asks");
        assert!(app.form_move(1));
        assert!(app.form_move(1));
        assert!(app.form_move(-1));
        typed(&app, "");
        let answer = sent(&mut receiver);
        assert_eq!(answer.action, FormAction::Accept);
        assert_eq!(
            serde_json::Value::Object(answer.content.unwrap()),
            json!({"Which database?": "SQLite"})
        );
        assert!(app.form.peek().is_none());
    });
}

#[test]
fn model_question_toggles_several_and_takes_a_typed_answer() {
    Runtime::scope("form-model-many", || {
        let workspace = tempfile::tempdir().unwrap();
        let form = Form {
            server: None,
            message: "Targets".to_owned(),
            fields: vec![
                question(
                    "Targets",
                    FieldKind::Choices {
                        options: choices(&["macOS", "Linux", "Windows"]),
                        min_items: None,
                        max_items: None,
                        custom: true,
                    },
                ),
                question(
                    "Shell",
                    FieldKind::Choice {
                        options: choices(&["zsh", "bash"]),
                        custom: true,
                    },
                ),
            ],
        };
        let (app, mut receiver) = opened(workspace.path(), form.clone());
        typed(&app, "");
        app.form_reply(Reply::Edit);
        assert!(app.form_toggle());
        assert!(app.form_move(2));
        assert!(app.form_toggle());
        let lines = app.form.peek().unwrap().shown("");
        assert!(
            lines.iter().any(|line| line == "❯ [x] Windows"),
            "{lines:?}"
        );
        typed(&app, "");
        typed(&app, "fish, with starship");
        app.form_reply(Reply::Send);
        let answer = sent(&mut receiver);
        assert_eq!(
            serde_json::Value::Object(answer.content.unwrap()),
            json!({"Targets": ["macOS", "Windows"], "Shell": "fish, with starship"})
        );
        let fresh = tempfile::tempdir().unwrap();
        let (app, mut receiver) = opened(fresh.path(), form);
        typed(&app, "");
        typed(&app, "");
        app.form_reply(Reply::Send);
        assert_eq!(
            serde_json::Value::Object(sent(&mut receiver).content.unwrap()),
            json!({"Shell": "zsh"})
        );
    });
}

#[test]
fn a_question_gives_back_the_unsent_draft_when_it_closes() {
    Runtime::scope("form-draft", || {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::create_dir(workspace.path().join(".git")).unwrap();
        std::fs::write(workspace.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(100, 30);
        app.replace("my draft".to_owned());
        app.update(Msg::Event(Event::Elicit {
            form: Some(form()),
            dialog: 1,
        }));
        app.form_reply(Reply::Cancel);
        assert_eq!(app.draft.peek(), "my draft");
    });
}

#[test]
fn a_typed_answer_shows_once_and_the_picker_points_to_it() {
    Runtime::scope("form-typed-answer", || {
        let workspace = tempfile::tempdir().unwrap();
        let form = Form {
            server: None,
            message: "Zero".to_owned(),
            fields: vec![question(
                "Which zero values should trigger the ValueError?",
                FieldKind::Choice {
                    options: choices(&["only 0", "0 and 0.0"]),
                    custom: true,
                },
            )],
        };
        let (app, _receiver) = opened(workspace.path(), form);
        app.replace("any zero, and also reject NaN divisors".to_owned());
        let Some(crate::tui::sigil_view::chat::panels::Band::Form { lines, .. }) =
            crate::tui::sigil_view::chat::panels::band(&app)
        else {
            panic!("no question band");
        };
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("your typed answer below")),
            "{lines:?}"
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("reject NaN divisors")),
            "the answer shows twice: {lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.starts_with("❯ 1.")),
            "{lines:?}"
        );
    });
}
