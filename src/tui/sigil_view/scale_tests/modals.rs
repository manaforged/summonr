use std::collections::BTreeMap;

use sigil::reactive::Runtime;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::{App, Facet, Login};
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

fn sent(prepare: impl FnOnce(&App), act: impl FnOnce(&App)) -> Vec<Cmd> {
    Runtime::scope("summonr-modals", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(160, 40);
        prepare(&app);
        while sent.try_recv().is_ok() {}
        act(&app);
        std::iter::from_fn(|| sent.try_recv().ok()).collect()
    })
}

pub(super) fn select(app: &App, endpoint: &str) {
    let at = app
        .picker
        .shown
        .peek()
        .iter()
        .position(|seat| seat.endpoint == endpoint)
        .expect("row");
    app.picker.sel.set(at);
}

pub(super) fn rejected_openai(app: &App) {
    open(
        app,
        vec![
            (Seat::new("openai", "gpt-alpha"), Model::named("gpt-alpha")),
            (Seat::new("openai", "gpt-beta"), Model::named("gpt-beta")),
            (Seat::new("deepseek", "ds-flash"), Model::named("ds-flash")),
        ],
        &[("openai", Reach::KeyRejected), ("deepseek", Reach::Ready)],
    );
}

#[test]
fn enter_on_an_offline_provider_retries_it_instead_of_seating_it() {
    let sent = sent(
        |app| {
            open(
                app,
                vec![(
                    Seat::new("ollama", "gpt-oss:20b"),
                    Model::named("gpt-oss:20b"),
                )],
                &[("ollama", Reach::Offline)],
            );
            select(app, "ollama");
        },
        App::commit_model_edit,
    );
    assert!(
        !sent
            .iter()
            .any(|command| matches!(command, Cmd::SetSeat(..) | Cmd::SetModel(..))),
        "seated an offline provider"
    );
    assert!(sent.iter().any(
        |command| matches!(command, Cmd::ListModels { only: Some(name), .. } if name == "ollama")
    ));
}

#[test]
fn a_typed_model_id_on_a_provider_that_needs_a_model_is_seated() {
    let sent = sent(
        |app| {
            open(app, vec![], &[("custom", Reach::NeedsModel)]);
            app.replace_model_draft("custom/my-model".to_owned());
        },
        App::commit_model_edit,
    );
    assert!(sent.iter().any(
        |command| matches!(command, Cmd::SetSeat(name, model) if name == "custom" && model == "my-model")
    ));
}

#[test]
fn favoriting_a_provider_fix_row_pins_nothing() {
    let sent = sent(
        |app| {
            rejected_openai(app);
            select(app, "openai");
        },
        App::pin_model,
    );
    assert!(
        !sent.iter().any(|command| matches!(command, Cmd::Pin(..))),
        "pinned a model behind a fix row"
    );
}

#[test]
fn a_filter_chip_counts_the_rows_it_shows() {
    Runtime::scope("summonr-modals", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        rejected_openai(&app);
        let counts = app.facet_counts();
        let all = counts
            .iter()
            .find(|(facet, _)| *facet == Facet::All)
            .expect("all");
        assert_eq!(all.1, app.picker.shown.peek().len());
    });
}

#[test]
fn a_large_price_fits_its_column() {
    let screen = screen(160, 40, |app| {
        let fact = model("frontier", |model| {
            model.cost = Cost::Metered;
            model.price = Some(Price {
                input: 150.0,
                output: 600.0,
                cached: 0.0,
                cache_write: None,
            });
        });
        open(
            app,
            vec![(Seat::new("deepseek", "frontier"), fact)],
            &[("deepseek", Reach::Ready)],
        );
    });
    assert!(screen.contains("$150.00/$600.00"), "{screen}");
}

#[test]
fn a_long_sign_in_url_is_shortened_to_the_form() {
    let url = format!("https://auth.example.com/device?{}", "q".repeat(80));
    let screen = screen(200, 50, |app| {
        app.replace("/login".to_owned());
        app.submit();
        app.auth.step.set(Login::SignIn {
            text: String::new(),
            url: url.clone(),
            code: "ABCD-1234".to_owned(),
            asking: false,
            note: String::new(),
        });
    });
    let line = screen
        .lines()
        .find(|line| line.contains("1. Open"))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(line.contains('…'), "{screen}");
}

fn frame_rows(screen: &str) -> (Option<usize>, Option<usize>) {
    let top = screen.lines().position(|line| line.contains('┏'));
    let bottom = screen.lines().position(|line| line.contains('┗'));
    (top, bottom)
}

#[test]
fn the_model_dialog_keeps_its_frame_while_models_load() {
    let loading = screen(160, 40, |app| {
        app.replace("/model".to_owned());
        app.submit();
    });
    let loaded = screen(160, 40, |app| {
        open(
            app,
            vec![(Seat::new("deepseek", "ds-flash"), Model::named("ds-flash"))],
            &[("deepseek", Reach::Ready)],
        );
    });
    assert!(loading.contains("loading models"), "{loading}");
    assert_eq!(
        frame_rows(&loading),
        frame_rows(&loaded),
        "{loading}\n{loaded}"
    );
}

#[test]
fn the_key_step_keeps_the_providers_frame_and_list() {
    let list = screen(160, 40, |app| {
        app.replace("/login".to_owned());
        app.submit();
    });
    let key = screen(160, 40, |app| {
        app.replace("/login".to_owned());
        app.submit();
        app.auth.step.set(Login::Key(String::new()));
    });
    assert!(key.contains("paste or type the api key"), "{key}");
    assert!(key.contains("Search providers"), "{key}");
    assert_eq!(frame_rows(&list), frame_rows(&key), "{list}\n{key}");
}

#[test]
fn a_rejected_key_from_the_model_picker_opens_in_its_pane() {
    let screen = screen(160, 40, |app| {
        rejected_openai(app);
        select(app, "openai");
        app.commit_model_edit();
    });
    assert!(screen.contains("Search models"), "{screen}");
    assert!(screen.contains("sign in with"), "{screen}");
}
