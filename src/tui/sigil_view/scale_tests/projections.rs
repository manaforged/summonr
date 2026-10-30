use super::super::app::{App, Facet, Login};
use super::{State, busy_info, frame_with, screens};
use crate::catalog::Model;
use crate::endpoint::State as Reach;
use crate::recent::ModelId;
use crate::tui::{Msg, Snapshot};

fn models(app: &App, rows: &[(&str, &str)], groups: &[(&str, Reach)]) {
    app.update(Msg::Models {
        request: app.picker.request.get(),
        rows: rows
            .iter()
            .map(|(endpoint, id)| ModelId::new(*endpoint, *id))
            .collect(),
        facts: rows
            .iter()
            .map(|(endpoint, id)| (ModelId::new(*endpoint, *id), Model::named(*id)))
            .collect(),
        groups: groups
            .iter()
            .map(|(name, state)| ((*name).to_owned(), *state))
            .collect(),
        causes: Default::default(),
        pinned: 0,
        recent: 0,
        loading: false,
        error: String::new(),
    });
}

fn endpoint(name: &str) -> Snapshot {
    Snapshot {
        endpoint: name.to_owned(),
        model: "driver".to_owned(),
        ..Snapshot::default()
    }
}

#[test]
fn ready_models_follow_a_late_key_rejection() {
    let shown = screens(
        160,
        40,
        |app| {
            app.begin_model_edit();
            models(
                app,
                &[("openai", "blocked-later"), ("deepseek", "still-ready")],
                &[("openai", Reach::Ready), ("deepseek", Reach::Ready)],
            );
            app.pick_facet(Facet::Ready);
        },
        &[&|app: &App| {
            app.auth.open.set(true);
            app.auth.step.set(Login::Checking);
            app.update(Msg::LoginDone {
                name: "openai".to_owned(),
                result: Err("key rejected".to_owned()),
                rejected: true,
            });
            app.cancel_auth();
        }],
    );
    assert!(shown[0].contains("blocked-later"), "{}", shown[0]);
    assert!(!shown[1].contains("blocked-later"), "{}", shown[1]);
    assert!(shown[1].contains("still-ready"), "{}", shown[1]);
}

#[test]
fn model_completion_uses_the_changed_endpoint() {
    let mut completed = String::new();
    let shown = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        app.update(Msg::Settings(endpoint("first")));
        app.begin_model_edit();
        models(app, &[("first", "solar"), ("second", "luna")], &[]);
        app.cancel_model_edit();
        app.replace("/model lun".to_owned());
        app.update(Msg::Settings(endpoint("second")));
        assert!(app.complete(), "the model argument must complete");
        completed = app.draft.peek();
    });
    assert_eq!(completed, "/model luna", "{}", shown.join("\n"));
}

#[test]
fn a_completed_command_sees_a_late_catalog_update() {
    let mut completed = String::new();
    let shown = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        app.update(Msg::Settings(endpoint("first")));
        app.begin_model_edit();
        models(app, &[("first", "legacy")], &[]);
        app.cancel_model_edit();
        app.replace("/model".to_owned());
        assert!(app.complete(), "the command must enter argument mode");
        assert_eq!(app.draft.peek(), "/model ");
        app.begin_model_edit();
        models(app, &[("first", "discovered")], &[]);
        app.cancel_model_edit();
        assert!(app.complete(), "the new model argument must complete");
        completed = app.draft.peek();
    });
    assert_eq!(completed, "/model discovered", "{}", shown.join("\n"));
}
