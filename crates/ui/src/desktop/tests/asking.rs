use sigil::prelude::Role;

use super::harness::{drive, typed};
use super::in_window;
use summonr_agent::hub::{Cmd, Fact, Msg};
use summonr_agent::mcp::{Choice, Field, FieldKind, Form};

fn asked(persist: bool) -> impl FnOnce(&super::super::App) {
    move |app| {
        app.update(Msg::Fact(Fact::Event(summonr_agent::Event::Approval {
            detail: Some("write src/main.rs".to_owned()),
            persist,
            dialog: 7,
        })));
    }
}

fn answers(sent: &[Cmd]) -> Vec<(u64, String)> {
    sent.iter()
        .filter_map(|cmd| match cmd {
            Cmd::Answer(dialog, text) => Some((*dialog, text.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_write_request_offers_three_answers_and_two_saves_it() {
    if !in_window("asking::a_write_request_offers_three_answers_and_two_saves_it") {
        return;
    }
    let before = drive(asked(true), Vec::new(), |_| ());
    for label in ["Allow for this chat", "Always allow", "Don't allow"] {
        assert_eq!(
            before.named(Role::Button, label).len(),
            1,
            "{label}: {:?}",
            before.controls
        );
    }
    assert!(
        before.shows(|text| text.contains("write src/main.rs")),
        "{}",
        before.text()
    );
    let after = drive(asked(true), typed("2"), |app| app.draft.peek());
    assert_eq!(answers(&after.sent), vec![(7, "always".to_owned())]);
    assert_eq!(after.read, "");
    assert!(after.named(Role::Button, "Always allow").is_empty());
}

#[test]
fn a_one_time_request_has_no_always_and_a_number_past_the_list_types() {
    if !in_window("asking::a_one_time_request_has_no_always_and_a_number_past_the_list_types") {
        return;
    }
    let seen = drive(asked(false), typed("3"), |app| app.draft.peek());
    assert!(answers(&seen.sent).is_empty());
    assert_eq!(seen.read, "3");
    let allowed = drive(asked(false), typed("1"), |_| ());
    assert_eq!(answers(&allowed.sent), vec![(7, "y".to_owned())]);
}

#[test]
fn a_question_with_choices_answers_with_the_numbered_choice() {
    if !in_window("asking::a_question_with_choices_answers_with_the_numbered_choice") {
        return;
    }
    let options = ["red", "green", "blue"]
        .into_iter()
        .map(|value| Choice {
            value: value.to_owned(),
            title: value.to_owned(),
            description: None,
        })
        .collect();
    let form = Form {
        server: None,
        message: "Pick a colour".to_owned(),
        fields: vec![Field {
            name: "colour".to_owned(),
            title: "Colour".to_owned(),
            description: None,
            kind: FieldKind::Choice {
                options,
                custom: false,
            },
            required: true,
            default: None,
        }],
    };
    let seen = drive(
        move |app| {
            app.update(Msg::Fact(Fact::Event(summonr_agent::Event::Elicit {
                form: Some(form),
                dialog: 9,
            })))
        },
        typed("2"),
        |_| (),
    );
    let sent = answers(&seen.sent);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].0, 9);
    assert!(sent[0].1.contains("\"colour\":\"green\""), "{}", sent[0].1);
}
