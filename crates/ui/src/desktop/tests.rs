use sigil::prelude::HostMode;

use sigil::Host;
use sigil::app::FrameHooks;
use sigil::events::{Event, KeyPhase, LogicalKey};
use sigil::prelude::NodeBuilder;
use sigil::scene::SceneCommand;
use tokio::sync::mpsc::unbounded_channel;

use super::{App, root, thread};
use summonr_agent::catalog::Model;
use summonr_agent::hub::{Fact, Msg};
use summonr_agent::recent::ModelId;

mod alignment;
mod asking;
mod attention;
mod bench;
mod changes;
mod controls;
mod harness;
mod panels;
mod panes;
mod pins;
mod place;
mod proportions;
mod queue;
mod scripts;
mod settings;
mod setup;
mod ship;
mod side;
mod sidebar;
mod spaces;
mod terminals;
mod topics;
mod transcript;
mod usage;
mod window_chrome;
mod window_composer;
mod window_focus;
mod worktrees;

const VIEW: (f32, f32) = (1200.0, 800.0);
const WINDOW_RUN: &str = "SUMMONR_TEST_WINDOW";

fn in_window(test: &str) -> bool {
    if std::env::var_os(WINDOW_RUN).is_some() {
        crate::desktop::host::window_cells();
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

fn key(text: &str) -> Event {
    Event::key(LogicalKey::character(text), KeyPhase::Down).text(Some(text.into()))
}

fn fit(app: &App) {
    let cell = super::super::canvas::cell();
    app.resize(
        (VIEW.0 / cell.w).floor().max(1.0) as u16,
        (VIEW.1 / cell.h).floor().max(1.0) as u16,
    );
}

fn texts(
    setup: impl FnOnce(&App),
    view: impl Fn(App) -> NodeBuilder + 'static,
) -> Vec<(String, f32)> {
    sigil::reactive::Owner::scope("summonr-desktop", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(
            super::super::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        fit(&app);
        setup(&app);
        app.refresh();
        let mut host = Host::new(HostMode::Pixel);
        host.mount(move || view(app.clone()));
        for _ in 0..3 {
            host.frame(VIEW, 1.0, FrameHooks::default());
        }
        host.scene(VIEW)
            .expect("scene")
            .commands
            .into_iter()
            .filter_map(|command| {
                let SceneCommand::GlyphRun(draw) = command else {
                    return None;
                };
                Some((draw.text().to_owned(), draw.rect.origin.y))
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
fn the_window_model_picker_rows_sit_between_header_and_hints() {
    if !in_window("the_window_model_picker_rows_sit_between_header_and_hints") {
        return;
    }
    let names = ["model-a", "model-b", "model-c", "model-d", "model-e"];
    let runs = texts(
        |app| {
            app.begin_model_edit();
            let selection = |id: &str| ModelId::new("openrouter", id);
            app.update(Msg::Fact(Fact::Models {
                request: 1,
                rows: names.iter().map(|id| selection(id)).collect(),
                facts: std::sync::Arc::new(
                    names
                        .iter()
                        .map(|id| (selection(id), Model::named(*id)))
                        .collect(),
                ),
                groups: Default::default(),
                causes: Default::default(),
                pinned: 0,
                recent: 0,
                loading: false,
                error: String::new(),
            }));
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
    let draft = sigil::reactive::Owner::scope("summonr-desktop-focus", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(
            super::super::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        app.refresh();
        let mut host = Host::new(HostMode::Pixel);
        let tree = app.clone();
        host.mount(move || root(tree.clone()));
        host.frame(VIEW, 0.0, FrameHooks::default());
        host.route(key("x"));
        host.frame(VIEW, 0.0, FrameHooks::default());
        app.draft.peek()
    });
    assert_eq!(draft, "x");
}
