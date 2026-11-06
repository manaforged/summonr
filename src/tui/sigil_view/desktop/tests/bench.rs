use std::time::Instant;

use sigil::Host;
use sigil::app::FrameHooks;
use sigil::prelude::HostMode;
use tokio::sync::mpsc::unbounded_channel;

use super::super::{App, root, split};
use super::VIEW;
use crate::tools::Event;
use crate::tui::sigil_view::view::Split;
use crate::tui::{Activity, Fact, Intent, Msg};

const DELTAS: usize = 400;
const WARM: usize = 50;

fn stream(app: &App, n: usize) {
    app.update(Msg::Fact(Fact::Event(Event::Delta { text: format!(" word{n}") })));
    if n % 40 == 39 {
        app.update(Msg::Fact(Fact::Event(Event::Delta { text: "\n\n".to_owned() })));
    }
}

fn history(n: usize) -> Vec<Msg> {
    (0..n)
        .flat_map(|n| {
            [
                Msg::Intent(Intent::User(format!("Step {n}: check the parser"))),
                Msg::Fact(Fact::Event(Event::Assistant {
                    text: format!("Checked step {n}. The parser handles **nested** lists and `code` spans.\n\n- one\n- two"),
                })),
            ]
        })
        .collect()
}

fn talk(pane: &App, n: usize) {
    history(n).into_iter().for_each(|msg| pane.update(msg));
}

fn run(panes: usize, streaming: usize, history: usize) -> f64 {
    sigil::reactive::Owner::scope("summonr-bench", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(crate::tui::sigil_view::profile::info(), workspace.path().to_owned(), commands);
        super::fit(&app);
        app.refresh();
        let mut host = Host::new(HostMode::Pixel);
        let tree = app.clone();
        host.mount(move || root(tree.clone()));
        host.frame(VIEW, 1.0, FrameHooks::default());
        for _ in 1..panes {
            split::act(Split::Open);
            host.frame(VIEW, 1.0, FrameHooks::default());
        }
        let apps = split::apps();
        assert_eq!(apps.len(), panes, "panes opened");
        let live: Vec<App> = apps.iter().take(streaming).cloned().collect();
        for pane in &live {
            talk(pane, history);
            pane.update(Msg::Intent(Intent::User("Review the change".to_owned())));
            pane.update(Msg::Fact(Fact::Busy(Some(Activity::Turn))));
            pane.refresh();
        }
        let mut step = |n: usize| {
            for pane in &live {
                stream(pane, n);
                pane.refresh();
            }
            host.frame(VIEW, 1.0, FrameHooks::default());
        };
        (0..WARM).for_each(&mut step);
        let wall = Instant::now();
        (WARM..WARM + DELTAS).for_each(&mut step);
        wall.elapsed().as_secs_f64() * 1e6 / DELTAS as f64
    })
}

#[test]
#[ignore = "bench: run alone with --ignored --exact"]
fn bench_stream_frames() {
    crate::tui::sigil_view::canvas::window_cells();
    let side = std::env::var("SUMMONR_BENCH_SIDE").unwrap_or_else(|_| "base".to_owned());
    for (cell, panes, streaming, history) in [("idle", 4, 0, 0), ("one", 1, 1, 0), ("four-one", 4, 1, 0), ("four-all", 4, 4, 0), ("long", 1, 1, 200)] {
        let trace = std::env::var_os("SUMMONR_BENCH_TRACE").filter(|_| cell == "four-all");
        if trace.is_some() {
            sigil::profile::reset();
            sigil::profile::enable();
        }
        let wall = run(panes, streaming, history);
        if let Some(path) = trace {
            sigil::profile::dump(path).expect("trace");
            sigil::profile::disable();
        }
        println!("RAW {cell} {side} us_frame {wall:.1}");
    }
}

/// One navigation step in a window showing one agent with `HISTORY` earlier messages.
#[derive(Clone, Copy)]
enum Step {
    Scroll,
    Settings,
    Switch(usize),
}

const HISTORY: usize = 100;
const LONG_HISTORY: usize = 1000;
const MOVES: usize = 60;
const MOVES_WARM: usize = 10;

fn wheel(n: usize) -> sigil::events::Event {
    use sigil::events::{Event, ScrollDelta, ScrollPhase};
    let phase = match n % 20 {
        0 => ScrollPhase::Started,
        19 => ScrollPhase::Ended,
        _ => ScrollPhase::Moved,
    };
    let dy = if (n / 20) % 2 == 0 { 12.0 } else { -12.0 };
    Event::scroll(
        sigil::geometry::Point::new(VIEW.0 * 0.6, VIEW.1 / 2.0),
        ScrollDelta::Pixels(sigil::geometry::Point::new(0.0, dy)),
        phase,
    )
}

fn navigate(step: Step) -> f64 {
    let mut runtime = sigil::tasks::AsyncRuntime::new().expect("runtime");
    runtime.install_current();
    sigil::reactive::Owner::scope("summonr-bench", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(crate::tui::sigil_view::profile::info(), workspace.path().to_owned(), commands);
        super::fit(&app);
        talk(&app, HISTORY);
        app.refresh();
        let mut host = Host::new(HostMode::Pixel);
        let tree = app.clone();
        host.mount(move || root(tree.clone()));
        host.frame(VIEW, 1.0, FrameHooks::default());
        let (hub, inbox) = unbounded_channel();
        let landed = std::rc::Rc::new(std::cell::Cell::new(0_usize));
        let (feed, seen) = (app.clone(), landed.clone());
        sigil::tasks::on_message(
            inbox,
            move |msg: Msg| {
                let done = matches!(msg, Msg::Fact(Fact::Replayed));
                feed.update(msg);
                if done {
                    seen.set(seen.get() + 1);
                }
            },
            || {},
        );
        let mut act = |n: usize| {
            match step {
                Step::Scroll => host.route(wheel(n)),
                Step::Settings => app.settings_open.set(n % 2 == 0),
                Step::Switch(messages) => {
                    let want = landed.get() + 1;
                    drop(hub.send(Msg::Intent(Intent::Reset)));
                    history(messages).into_iter().for_each(|msg| drop(hub.send(msg)));
                    drop(hub.send(Msg::Fact(Fact::Replayed)));
                    while landed.get() < want {
                        runtime.pump();
                    }
                    app.refresh();
                }
            }
            host.frame(VIEW, 1.0, FrameHooks::default());
        };
        (0..MOVES_WARM).for_each(&mut act);
        let wall = Instant::now();
        (MOVES_WARM..MOVES_WARM + MOVES).for_each(&mut act);
        wall.elapsed().as_secs_f64() * 1e6 / MOVES as f64
    })
}

#[test]
#[ignore = "bench: run alone with --ignored --exact"]
fn bench_navigate_frames() {
    crate::tui::sigil_view::canvas::window_cells();
    let side = std::env::var("SUMMONR_BENCH_SIDE").unwrap_or_else(|_| "base".to_owned());
    for (cell, step) in [
        ("scroll", Step::Scroll),
        ("settings", Step::Settings),
        ("switch", Step::Switch(HISTORY)),
        ("switch-long", Step::Switch(LONG_HISTORY)),
    ] {
        let trace = std::env::var_os("SUMMONR_BENCH_TRACE").filter(|_| cell == "switch");
        if trace.is_some() {
            sigil::profile::reset();
            sigil::profile::enable();
        }
        let wall = navigate(step);
        if let Some(path) = trace {
            sigil::profile::dump(path).expect("trace");
            sigil::profile::disable();
        }
        println!("RAW {cell} {side} us_frame {wall:.1}");
    }
}
