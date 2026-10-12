use sigil::Host;
use sigil::kernel::NodeBuilder;
use sigil::reactive::Runtime;
use sigil::scene::SceneCommand;
use tokio::sync::mpsc::unbounded_channel;

use super::{App, rail, thread};
use crate::tools::Event;
use crate::tui::hub::{Live, Status};
use crate::tui::{Msg, Snapshot};

const VIEW: (f32, f32) = (1200.0, 800.0);

fn texts(
    setup: impl FnOnce(&App),
    view: impl Fn(App) -> NodeBuilder + 'static,
) -> Vec<(String, f32)> {
    Runtime::scope("summonr-desktop", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(
            super::super::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        setup(&app);
        app.refresh();
        let mut host = Host::new();
        host.mount(move || view(app.clone()));
        host.scene(VIEW)
            .expect("scene")
            .commands
            .into_iter()
            .filter_map(|command| {
                let SceneCommand::GlyphRun(draw) = command else {
                    return None;
                };
                Some((draw.run.text.clone(), draw.run.rect.origin.y))
            })
            .collect()
    })
}

fn shown(runs: &[(String, f32)]) -> String {
    runs.iter()
        .map(|(text, _)| text.as_str())
        .collect::<Vec<_>>()
        .join(" | ")
}

fn row(runs: &[(String, f32)], text: &str) -> f32 {
    runs.iter()
        .find(|(run, _)| run == text)
        .map(|(_, y)| *y)
        .unwrap_or_else(|| panic!("no {text:?} in {}", shown(runs)))
}

#[test]
fn the_context_meter_draws_no_caption_text() {
    let runs = texts(
        |app| {
            app.update(Msg::Settings(Snapshot {
                model: "stub-model".to_owned(),
                context_window: Some(200_000),
                ..Snapshot::default()
            }));
            app.update(Msg::Event(Event::Usage {
                prompt_tokens: 1500,
                completion_tokens: 20,
                reasoning_tokens: 0,
                cached_tokens: 0,
                millis: None,
                prefix_hash: String::new(),
                cost: None,
            }));
        },
        thread::status::meters,
    );
    assert!(
        runs.iter().any(|(text, _)| text == "1%"),
        "{}",
        shown(&runs)
    );
    assert!(
        runs.iter()
            .all(|(text, _)| text != thread::status::CONTEXT_USED),
        "{}",
        shown(&runs)
    );
}

#[test]
fn the_rail_names_the_ledger_pane_as_it_titles_it() {
    let runs = texts(|_| {}, rail::rail);
    row(&runs, rail::LEDGER);
    assert!(
        runs.iter().all(|(text, _)| text != "Plan"),
        "{}",
        shown(&runs)
    );
}

#[test]
fn the_sessions_heading_sits_a_gap_below_the_last_pane() {
    let runs = texts(|_| {}, rail::rail);
    let ledger = row(&runs, rail::LEDGER);
    let peers = row(&runs, "Peers");
    let heading = row(&runs, "Sessions");
    assert!(
        heading - ledger > ledger - peers,
        "Peers {peers}, Ledger {ledger}, Sessions {heading}"
    );
}

#[test]
fn an_untitled_live_session_is_not_named_like_the_new_session_button() {
    let runs = texts(
        |app| {
            app.update(Msg::Live(vec![Live {
                session: "0123456789abcdef".to_owned(),
                title: String::new(),
                workspace: std::path::PathBuf::from("/tmp/proj"),
                status: Status::Idle,
                active: true,
            }]));
        },
        rail::rail,
    );
    let buttons = runs
        .iter()
        .filter(|(text, _)| text == rail::NEW_SESSION)
        .count();
    assert_eq!(buttons, 1, "{}", shown(&runs));
    row(&runs, rail::UNTITLED);
}
