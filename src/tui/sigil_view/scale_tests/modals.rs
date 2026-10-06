use std::collections::BTreeMap;

use sigil::reactive::Runtime;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::{App, Facet};
use super::{State, busy_info, frame_with};
use crate::catalog::{Cost, Model};
use crate::endpoint::State as Reach;
use crate::provider::Price;
use crate::recent::Seat;
use crate::tui::{Cmd, Msg};

fn model(id: &str, edit: impl FnOnce(&mut Model)) -> Model {
    let mut model = Model::named(id);
    edit(&mut model);
    model
}

fn listed(app: &App, rows: Vec<(Seat, Model)>, groups: &[(&str, Reach)]) {
    app.update(Msg::Models {
        request: app.picker.request.get(),
        rows: rows.iter().map(|(seat, _)| seat.clone()).collect(),
        facts: rows.into_iter().collect(),
        groups: groups
            .iter()
            .map(|(name, state)| ((*name).to_owned(), *state))
            .collect(),
        causes: BTreeMap::new(),
        pinned: 0,
        recent: 0,
        loading: false,
        error: String::new(),
    });
}

fn open(app: &App, rows: Vec<(Seat, Model)>, groups: &[(&str, Reach)]) {
    app.replace("/model".to_owned());
    app.submit();
    listed(app, rows, groups);
}

fn screen(cols: u16, rows: u16, setup: impl FnOnce(&App)) -> String {
    frame_with(busy_info(), cols, rows, State::Idle, setup).join("\n")
}

#[test]
fn a_provider_that_rejected_its_key_is_one_row_with_its_fix() {
    let screen = screen(160, 40, |app| {
        open(
            app,
            vec![
                (Seat::new("openai", "gpt-alpha"), Model::named("gpt-alpha")),
                (Seat::new("openai", "gpt-beta"), Model::named("gpt-beta")),
                (Seat::new("deepseek", "ds-flash"), Model::named("ds-flash")),
            ],
            &[("openai", Reach::KeyRejected), ("deepseek", Reach::Ready)],
        );
    });
    assert!(screen.contains("needs attention"), "{screen}");
    assert!(screen.contains("key rejected · ↵ new key"), "{screen}");
    assert!(!screen.contains("gpt-beta"), "{screen}");
    assert!(screen.contains("ds-flash"), "{screen}");
}

#[test]
fn the_model_picker_shows_dollar_prices_per_million_tokens() {
    let screen = screen(160, 40, |app| {
        let fact = model("metered-model", |model| {
            model.cost = Cost::Metered;
            model.price = Some(Price {
                input: 3.0,
                output: 15.0,
                cached: 0.3,
                cache_write: None,
            });
        });
        open(
            app,
            vec![(Seat::new("deepseek", "metered-model"), fact)],
            &[("deepseek", Reach::Ready)],
        );
        app.overlay_move(1);
    });
    assert!(screen.contains("$3.00/$15.00"), "{screen}");
    assert!(
        screen.contains("$3.00 in · $15.00 out · $0.30 cached"),
        "{screen}"
    );
}

#[test]
fn the_think_filter_keeps_only_reasoning_models() {
    let screen = screen(160, 40, |app| {
        open(
            app,
            vec![
                (
                    Seat::new("deepseek", "reasoner"),
                    model("reasoner", |model| model.reasoning = true),
                ),
                (
                    Seat::new("deepseek", "plain-chat"),
                    Model::named("plain-chat"),
                ),
            ],
            &[("deepseek", Reach::Ready)],
        );
        app.pick_facet(Facet::Think);
    });
    assert!(screen.contains("reasoner"), "{screen}");
    assert!(!screen.contains("plain-chat"), "{screen}");
}

#[test]
fn the_providers_dialog_lists_every_provider_on_a_tall_terminal() {
    let screen = screen(200, 60, |app| {
        app.replace("/login".to_owned());
        app.submit();
    });
    let rows = screen
        .lines()
        .filter(|line| line.contains("needs key") || line.contains("sign in"))
        .count();
    assert!(rows > 11, "{rows} provider rows:\n{screen}");
}

#[test]
fn ctrl_right_from_models_opens_providers() {
    let screen = screen(160, 40, |app| {
        open(app, vec![], &[]);
        assert!(app.step_tab(1));
    });
    assert!(screen.contains("Search providers"), "{screen}");
    assert!(!screen.contains("Search models"), "{screen}");
}

#[test]
fn enter_on_an_offline_provider_retries_it_instead_of_seating_it() {
    Runtime::scope("summonr-modals", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(160, 40);
        open(
            &app,
            vec![(
                Seat::new("ollama", "gpt-oss:20b"),
                Model::named("gpt-oss:20b"),
            )],
            &[("ollama", Reach::Offline)],
        );
        let offline = app
            .picker
            .shown
            .peek()
            .iter()
            .position(|seat| seat.endpoint == "ollama")
            .expect("offline row");
        app.picker.sel.set(offline);
        while sent.try_recv().is_ok() {}
        app.commit_model_edit();
        let mut retried = None;
        while let Ok(command) = sent.try_recv() {
            assert!(
                !matches!(command, Cmd::SetSeat(..) | Cmd::SetModel(..)),
                "seated an offline provider"
            );
            if let Cmd::ListModels { only, .. } = command {
                retried = only;
            }
        }
        assert_eq!(retried.as_deref(), Some("ollama"));
    });
}
