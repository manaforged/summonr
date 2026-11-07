use std::time::Instant;

use sigil::Host;
use sigil::app::FrameHooks;
use sigil::prelude::HostMode;
use tokio::sync::mpsc::unbounded_channel;

use super::super::{App, root, split};
use super::VIEW;
use crate::view::keys::bindings::Split;
use summonr_agent::hub::{Activity, Fact, Intent, Msg};
use summonr_agent::tools::Event;

const DELTAS: usize = 400;
const WARM: usize = 50;

fn stream(app: &App, n: usize) {
    app.update(Msg::Fact(Fact::Event(Event::Delta {
        text: format!(" word{n}"),
    })));
    if n % 40 == 39 {
        app.update(Msg::Fact(Fact::Event(Event::Delta {
            text: "\n\n".to_owned(),
        })));
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
        let app = App::new(
            crate::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
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
    crate::desktop::host::window_cells();
    let side = std::env::var("SUMMONR_BENCH_SIDE").unwrap_or_else(|_| "base".to_owned());
    for (cell, panes, streaming, history) in [
        ("idle", 4, 0, 0),
        ("one", 1, 1, 0),
        ("four-one", 4, 1, 0),
        ("four-all", 4, 4, 0),
        ("long", 1, 1, 200),
    ] {
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
    Hover(usize),
    Drag(usize),
    DragBig,
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

fn over_text(n: usize) -> sigil::geometry::Point {
    let along = (n % 40) as f32 * 6.0;
    sigil::geometry::Point::new(VIEW.0 * 0.45 + along, VIEW.1 * 0.25 + along)
}

fn navigate(step: Step) -> f64 {
    let mut runtime = sigil::tasks::AsyncRuntime::new().expect("runtime");
    runtime.install_current();
    sigil::reactive::Owner::scope("summonr-bench", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(
            crate::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        super::fit(&app);
        match step {
            Step::Hover(depth) | Step::Drag(depth) => talk(&app, depth),
            Step::DragBig => {
                let para = "The parser handles **nested** lists and `code` spans, and it keeps every word in order across this long paragraph of an answer. ".repeat(6);
                let text = vec![para; 60].join("\n\n");
                for n in 0..6 {
                    app.update(Msg::Intent(Intent::User(format!("Step {n}"))));
                    app.update(Msg::Fact(Fact::Event(Event::Assistant {
                        text: text.clone(),
                    })));
                }
            }
            Step::Scroll | Step::Settings | Step::Switch(_) => talk(&app, HISTORY),
        }
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
        let (scene_us, a11y_us) = (std::cell::Cell::new(0.0), std::cell::Cell::new(0.0));
        let mut act = |n: usize| {
            match step {
                Step::Scroll => host.route(wheel(n)),
                Step::Settings => app.settings_open.set(n % 2 == 0),
                Step::Hover(_) => host.route(sigil::events::Event::pointer_move(over_text(n))),
                Step::Drag(_) | Step::DragBig if n == 0 => {
                    host.route(sigil::events::Event::pointer_down(over_text(n)))
                }
                Step::Drag(_) | Step::DragBig => {
                    host.route(sigil::events::Event::pointer_move(over_text(n)))
                }
                Step::Switch(messages) => {
                    let want = landed.get() + 1;
                    let mut lines = vec![Msg::Intent(Intent::Reset)];
                    lines.extend(history(messages));
                    drop(hub.send(Msg::Fact(Fact::Transcript(lines))));
                    drop(hub.send(Msg::Fact(Fact::Replayed)));
                    while landed.get() < want {
                        runtime.pump();
                    }
                    app.refresh();
                }
            }
            host.frame(VIEW, 1.0, FrameHooks::default());
            if matches!(step, Step::Hover(_) | Step::Drag(_) | Step::DragBig) {
                let mark = Instant::now();
                drop(host.scene(VIEW));
                scene_us.set(scene_us.get() + mark.elapsed().as_secs_f64() * 1e6);
                let mark = Instant::now();
                drop(host.accessibility_tree());
                a11y_us.set(a11y_us.get() + mark.elapsed().as_secs_f64() * 1e6);
            }
        };
        (0..MOVES_WARM).for_each(&mut act);
        let wall = Instant::now();
        scene_us.set(0.0);
        a11y_us.set(0.0);
        (MOVES_WARM..MOVES_WARM + MOVES).for_each(&mut act);
        println!(
            "SPLIT scene {:.1} a11y {:.1}",
            scene_us.get() / MOVES as f64,
            a11y_us.get() / MOVES as f64
        );
        let spent = wall.elapsed().as_secs_f64() * 1e6 / MOVES as f64;
        spent
    })
}

#[test]
#[ignore = "bench: run alone with --ignored --exact"]
fn bench_navigate_frames() {
    crate::desktop::host::window_cells();
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

#[test]
#[ignore = "bench: run alone with --ignored --exact"]
fn bench_drag_frames() {
    crate::desktop::host::window_cells();
    let side = std::env::var("SUMMONR_BENCH_SIDE").unwrap_or_else(|_| "base".to_owned());
    for (cell, step) in [
        ("hover", Step::Hover(HISTORY)),
        ("drag", Step::Drag(HISTORY)),
        ("hover-long", Step::Hover(LONG_HISTORY)),
        ("drag-long", Step::Drag(LONG_HISTORY)),
        ("drag-big", Step::DragBig),
    ] {
        let wall = navigate(step);
        println!("RAW {cell} {side} us_frame {wall:.1}");
    }
}

#[test]
#[ignore = "bench: run alone with --ignored --exact"]
fn bench_paint_frames() {
    use sigil::renderer::capture::{ImageRenderer, ImageTarget, render_to_image};
    crate::desktop::host::window_cells();
    let viewport = (1728.0, 1117.0);
    let mut runtime = sigil::tasks::AsyncRuntime::new().expect("runtime");
    runtime.install_current();
    sigil::reactive::Owner::scope("summonr-bench", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(
            crate::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        super::fit(&app);
        talk(&app, HISTORY);
        app.refresh();
        let mut host = Host::new(HostMode::Pixel);
        let tree = app.clone();
        host.mount(move || root(tree.clone()));
        host.frame(viewport, 2.0, FrameHooks::default());
        host.route(sigil::events::Event::pointer_down(
            sigil::geometry::Point::new(700.0, 300.0),
        ));
        host.frame(viewport, 2.0, FrameHooks::default());
        host.route(sigil::events::Event::pointer_move(
            sigil::geometry::Point::new(900.0, 700.0),
        ));
        host.frame(viewport, 2.0, FrameHooks::default());
        let full = host.scene(viewport).expect("scene");
        let blank = sigil::scene::CommandScene::default();
        let mut renderer = sigil::renderer::WgpuRenderer::new();
        let target = ImageTarget {
            size: sigil::geometry::PhysicalSize::new(3456, 2234),
            viewport,
            clear: sigil::style::Color::WHITE,
        };
        let mut time = |scene: &sigil::scene::CommandScene| {
            for _ in 0..5 {
                drop(render_to_image(
                    ImageRenderer::Gpu(&mut renderer),
                    scene,
                    target,
                ));
            }
            let start = Instant::now();
            for _ in 0..20 {
                render_to_image(ImageRenderer::Gpu(&mut renderer), scene, target).expect("paint");
            }
            start.elapsed().as_secs_f64() * 1e3 / 20.0
        };
        let (blank_ms, full_ms) = (time(&blank), time(&full));
        println!(
            "RAW paint commands {} blank_ms {blank_ms:.2} full_ms {full_ms:.2} paint_ms {:.2}",
            full.commands.len(),
            full_ms - blank_ms
        );
    });
}
