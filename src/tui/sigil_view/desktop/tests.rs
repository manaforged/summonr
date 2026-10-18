use std::collections::BTreeMap;

use sigil::Host;
use sigil::app::FrameHooks;
use sigil::events::{
    EventRouter, KeyEvent, KeyEventInit, KeyPhase, LogicalKey, ModifiersState, PhysicalKey,
    ShellEvent,
};
use sigil::kernel::NodeBuilder;
use sigil::reactive::Runtime;
use sigil::scene::SceneCommand;
use tokio::sync::mpsc::unbounded_channel;

use super::{App, rail, root, thread};
use crate::catalog::Model;
use crate::recent::Seat;
use crate::tools::Event;
use crate::tui::hub::{Live, Status};
use crate::tui::{Msg, Snapshot};

const VIEW: (f32, f32) = (1200.0, 800.0);
const WINDOW_RUN: &str = "SUMMONR_TEST_WINDOW";

fn in_window(test: &str) -> bool {
    if std::env::var_os(WINDOW_RUN).is_some() {
        crate::tui::sigil_view::canvas::window_cells();
        return true;
    }
    let path = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, rest)| rest);
    let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([&format!("{path}::{test}"), "--exact", "--test-threads=1"])
        .env(WINDOW_RUN, "1")
        .status()
        .expect("window run");
    assert!(status.success(), "{test} failed in window mode");
    false
}

fn key(text: &str) -> ShellEvent {
    ShellEvent::Key(KeyEvent::new(KeyEventInit {
        key: LogicalKey::character(text),
        physical: PhysicalKey::Unidentified,
        modifiers: ModifiersState::empty(),
        repeat: false,
        phase: KeyPhase::Down,
        text: Some(text.into()),
    }))
}

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
        let mut router = EventRouter::new();
        for _ in 0..3 {
            host.frame(&mut router, VIEW, 1.0, FrameHooks::default());
        }
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

#[test]
fn recent_leaves_out_sessions_without_a_message() {
    let info = |id: &str, preview: &str| crate::session::SessionInfo {
        id: id.to_owned(),
        path: std::path::PathBuf::new(),
        cwd: Some("/work".to_owned()),
        model: None,
        preview: preview.to_owned(),
        name: None,
        parent: None,
        depth: 0,
    };
    let shown = rail::recent(
        vec![info("empty", ""), info("used", "fix the tests")],
        "/work",
    );
    let ids: Vec<&str> = shown.iter().map(|info| info.id.as_str()).collect();
    assert_eq!(ids, ["used"]);
}

#[test]
fn the_window_model_picker_rows_sit_between_header_and_hints() {
    if !in_window("the_window_model_picker_rows_sit_between_header_and_hints") {
        return;
    }
    let names = ["model-a", "model-b", "model-c", "model-d", "model-e"];
    let runs = texts(
        |app| {
            app.begin_model_edit();
            let seat = |id: &str| Seat::new("openrouter", id);
            app.update(Msg::Models {
                request: 1,
                rows: names.iter().map(|id| seat(id)).collect(),
                facts: names
                    .iter()
                    .map(|id| (seat(id), Model::named(*id)))
                    .collect(),
                groups: BTreeMap::new(),
                causes: BTreeMap::new(),
                pinned: 0,
                recent: 0,
                loading: false,
                error: String::new(),
            });
        },
        root,
    );
    let header = row(&runs, "MODEL");
    let hints = runs
        .iter()
        .filter(|(text, _)| text.contains("refresh"))
        .map(|(_, y)| *y)
        .fold(f32::MAX, f32::min);
    for name in names {
        let at = row(&runs, name);
        assert!(
            header < at && at < hints,
            "{name} at {at}, header {header}, hints {hints}: {}",
            shown(&runs)
        );
    }
}

#[test]
fn a_slash_in_the_window_composer_lists_commands() {
    let runs = texts(|app| app.replace("/sett".to_owned()), thread::main_col);
    assert!(
        runs.iter().any(|(text, _)| text == "commands")
            && runs.iter().any(|(text, _)| text.starts_with("/settings")),
        "{}",
        shown(&runs)
    );
}

#[test]
fn the_window_composer_takes_typing_at_launch() {
    let draft = Runtime::scope("summonr-desktop-focus", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(
            super::super::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        app.refresh();
        let mut host = Host::new();
        let tree = app.clone();
        host.mount(move || root(tree.clone()));
        let mut router = EventRouter::new();
        host.frame(&mut router, VIEW, 0.0, FrameHooks::default());
        host.route(&mut router, key("x"));
        host.frame(&mut router, VIEW, 0.0, FrameHooks::default());
        app.draft.peek()
    });
    assert_eq!(draft, "x");
}
