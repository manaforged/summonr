use std::env;
use std::path::PathBuf;

use sigil::prelude::Cx;
use sigil::profile;
use sigil::reactive::Runtime;
use sigil_tui::{EventRouter, Grid, Host, Screen, render};
use tokio::runtime::Handle;
use tokio::sync::mpsc::unbounded_channel;
use tokio::task::block_in_place;
use tokio::time::sleep;

use super::app::App;
use super::host::frame_budget;
use super::view::root;
use crate::durable::store::write;
use crate::error::{Error, Result};
use crate::prefs::{FPS_DEFAULT, FPS_MAX, FPS_MIN, Prefs};
use crate::tools::Event;
use crate::tui::{Info, Msg};

struct Options {
    scenario: Scenario,
    frames: u16,
    history: u16,
    cols: u16,
    rows: u16,
    fps: u32,
    output: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Idle = 1,
    Stream,
    Resize,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut args = args.peekable();
        let scenario = if args.peek().is_some_and(|arg| !arg.starts_with('-')) {
            args.next().unwrap_or_default()
        } else {
            String::from("idle")
        };
        let mut options = Self {
            scenario: match scenario.as_str() {
                "idle" => Scenario::Idle,
                "stream" => Scenario::Stream,
                "resize" => Scenario::Resize,
                unknown => {
                    return Err(Error::Config(format!("unknown profile scenario {unknown}")));
                }
            },
            frames: 120,
            history: 100,
            cols: 120,
            rows: 40,
            fps: FPS_DEFAULT,
            output: env::var_os("SIGIL_PROFILE")
                .filter(|value| value != "1")
                .map_or_else(|| PathBuf::from("sigil-profile.json"), PathBuf::from),
        };
        while let Some(name) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| Error::Config(format!("missing value for {name}")))?;
            let number = || {
                value
                    .parse::<u16>()
                    .map_err(|error| Error::Config(format!("invalid {name}: {error}")))
            };
            match name.as_str() {
                "--frames" => options.frames = number()?,
                "--history" => options.history = number()?,
                "--cols" => options.cols = number()?,
                "--rows" => options.rows = number()?,
                "--fps" => options.fps = u32::from(number()?),
                "--output" => options.output = value.into(),
                unknown => return Err(Error::Config(format!("unknown profile option {unknown}"))),
            }
        }
        if !(1..=120).contains(&options.frames)
            || options.history > 1000
            || !(40..=512).contains(&options.cols)
            || !(10..=200).contains(&options.rows)
            || !(FPS_MIN..=FPS_MAX).contains(&options.fps)
        {
            return Err(Error::Config(format!(
                "profile-tui: idle|stream|resize; frames 1..120, history 0..1000, cols 40..512, rows 10..200, fps {FPS_MIN}..{FPS_MAX}",
            )));
        }
        Ok(options)
    }

    fn cause(&self) -> &'static str {
        match self.scenario {
            Scenario::Idle => "tui.idle",
            Scenario::Stream => "tui.stream",
            Scenario::Resize => "tui.resize",
        }
    }
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let options = Options::parse(args)?;
    block_in_place(|| {
        Runtime::scope("summonr-profile", || {
            Handle::current().block_on(capture(&options))
        })
    })
}

async fn capture(options: &Options) -> Result<()> {
    let workspace = tempfile::tempdir()?;
    let (commands, _receiver) = unbounded_channel();
    let app = App::new(info(), workspace.path().to_owned(), commands);
    app.resize(options.cols, options.rows);
    for index in 0..options.history {
        app.update(Msg::User(format!("Review change {index}")));
        app.update(Msg::Event(Event::Assistant {
            text: format!("Change {index}: measured layout, terminal output, and memory usage."),
        }));
    }
    app.refresh();
    let mut host = Host::cells();
    let tree = app.clone();
    host.mount(move |cx: &mut Cx| cx.add(root(tree)));
    let mut screen = Screen::new(options.cols, options.rows);
    let mut router = EventRouter::new();
    let mut output = Vec::new();
    profile::enable();
    profile::reset();
    let mut size = (options.cols, options.rows);
    for index in 0..options.frames {
        profile::begin_frame();
        profile::interact(options.cause());
        let frame = profile::span("frame");
        size = update(&app, options, index);
        output.clear();
        paint(&mut screen, &mut host, &mut router, size, &mut output)?;
        counts(options, size, output.len());
        drop(frame);
        profile::tick_frame();
        sleep(frame_budget(options.fps)).await;
    }
    profile::dump(&options.output)?;
    profile::disable();
    let grid = screen
        .frame(&mut host, &mut router, [], size)
        .map_err(|error| Error::Tool(error.to_string()))?;
    let mut ansi = Vec::new();
    render(&mut ansi, &Grid::new(0, 0), grid)?;
    write(&options.output.with_extension("ansi"), ansi)?;
    write(&options.output.with_extension("txt"), grid.text())?;
    Ok(())
}

fn paint(
    screen: &mut Screen,
    host: &mut Host,
    router: &mut EventRouter,
    size: (u16, u16),
    output: &mut Vec<u8>,
) -> Result<()> {
    {
        let _phase = profile::span("tui.frame");
        screen
            .frame(host, router, [], size)
            .map_err(|error| Error::Tool(error.to_string()))?;
    }
    let _phase = profile::span("tui.write");
    screen.flush(output)?;
    Ok(())
}

fn update(app: &App, options: &Options, index: u16) -> (u16, u16) {
    let _phase = profile::span("app.update");
    let width = if options.scenario == Scenario::Resize && index.is_multiple_of(2) {
        options.cols - 16
    } else {
        options.cols
    };
    if options.scenario == Scenario::Resize {
        app.resize(width, options.rows);
    } else if options.scenario == Scenario::Stream {
        app.update(Msg::Event(Event::Delta {
            text: format!(" token{index}"),
        }));
        app.refresh();
    }
    (width, options.rows)
}

fn counts(options: &Options, size: (u16, u16), bytes: usize) {
    for (name, value) in [
        ("tui.frames", 1),
        ("tui.bytes", bytes as u64),
        ("workload.cols", u64::from(size.0)),
        ("workload.rows", u64::from(size.1)),
        ("workload.items", u64::from(options.history)),
        ("workload.headless", 1),
        ("workload.fps", u64::from(options.fps)),
        ("workload.scenario", options.scenario as u64),
        ("build.profile", if cfg!(debug_assertions) { 1 } else { 2 }),
    ] {
        profile::count_add(name, value);
    }
}

fn info() -> Info {
    Info {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        model: String::from("profile"),
        relay: None,
        session: String::from("profile"),
        endpoint: String::from("profile"),
        prefs: Prefs {
            animations: false,
            splash: false,
            ..Prefs::default()
        },
        splash: false,
        plan: false,
        strict: false,
        workspace: String::from("profile"),
        workflow: None,
        config: None,
    }
}
