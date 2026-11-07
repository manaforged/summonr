use std::env;
use std::path::PathBuf;

use sigil::Host;
use sigil::profile;
use sigil::terminal::{ColorDepth, Grid, Screen};
use tokio::runtime::Handle;
use tokio::sync::mpsc::unbounded_channel;
use tokio::task::block_in_place;
use tokio::time::sleep;

use super::app::App;
use super::view::root;
use summonr_agent::durable::store::{Durability, write};
use summonr_agent::hub::{Fact, Info, Intent, Msg};
use summonr_agent::prefs::{FPS_DEFAULT, FPS_MAX, FPS_MIN, Prefs};
use summonr_agent::tools::Event;
use summonr_core::error::{Error, Result};

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
    Help,
    Agents,
    AgentsFailed,
}

impl Scenario {
    fn parse(name: &str) -> Result<Self> {
        match name {
            "idle" => Ok(Self::Idle),
            "stream" => Ok(Self::Stream),
            "resize" => Ok(Self::Resize),
            "help" => Ok(Self::Help),
            "agents" => Ok(Self::Agents),
            "agents-failed" => Ok(Self::AgentsFailed),
            unknown => Err(Error::Config(format!("unknown profile scenario {unknown}"))),
        }
    }
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
            scenario: Scenario::parse(&scenario)?,
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
            options.set(&name, value)?;
        }
        options.validate()?;
        if options.output.extension().is_some_and(|ext| ext == "png") {
            if !crate::window::CAPTURES_IMAGE {
                return Err(Error::Config(String::from(
                    "terminal PNG capture requires a build with --features gui; no window is opened",
                )));
            }
            options.output.set_extension("json");
        }
        Ok(options)
    }

    fn set(&mut self, name: &str, value: String) -> Result<()> {
        let number = || {
            value
                .parse::<u16>()
                .map_err(|error| Error::Config(format!("invalid {name}: {error}")))
        };
        match name {
            "--frames" => self.frames = number()?,
            "--history" => self.history = number()?,
            "--cols" => self.cols = number()?,
            "--rows" => self.rows = number()?,
            "--fps" => self.fps = u32::from(number()?),
            "--output" => self.output = value.into(),
            unknown => return Err(Error::Config(format!("unknown profile option {unknown}"))),
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if !(1..=120).contains(&self.frames)
            || self.history > 1000
            || !(40..=512).contains(&self.cols)
            || !(10..=200).contains(&self.rows)
            || !(FPS_MIN..=FPS_MAX).contains(&self.fps)
        {
            return Err(Error::Config(format!(
                "profile-tui: idle|stream|resize|help|agents|agents-failed; frames 1..120, history 0..1000, cols 40..512, rows 10..200, fps {FPS_MIN}..{FPS_MAX}",
            )));
        }
        Ok(())
    }

    fn cause(&self) -> profile::Cause {
        match self.scenario {
            Scenario::Idle | Scenario::Help | Scenario::Agents | Scenario::AgentsFailed => {
                profile::Cause::Idle
            }
            Scenario::Stream => profile::Cause::Input,
            Scenario::Resize => profile::Cause::Resize,
        }
    }
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let options = Options::parse(args)?;
    block_in_place(|| {
        sigil::reactive::Owner::scope("summonr-profile", || {
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
        app.update(Msg::Intent(Intent::User(format!("Review change {index}"))));
        app.update(Msg::Fact(Fact::Event(Event::Assistant {
            text: format!("Change {index}: measured layout, terminal output, and memory usage."),
        })));
    }
    scenes::seed(&app, options.scenario);
    app.refresh();
    let mut host = Host::new(sigil::prelude::HostMode::Cell);
    let tree = app.clone();
    host.mount(move || root(tree));
    let mut screen = Screen::new(options.cols, options.rows);
    let mut output = Vec::new();
    profile::enable();
    profile::reset();
    let mut size = (options.cols, options.rows);
    for index in 0..options.frames {
        profile::begin_frame(profile::BackendStatus::TERMINAL);
        profile::interact(options.cause());
        size = update(&app, options, index);
        output.clear();
        paint(&mut screen, &mut host, size, &mut output)?;
        counts(options, size, output.len());
        profile::tick_frame();
        sleep(frame_budget(options.fps)).await;
    }
    profile::dump(&options.output).map_err(|error| Error::Tool(error.to_string()))?;
    profile::disable();
    let grid = screen
        .frame(&mut host, [], size)
        .map_err(|error| Error::Tool(error.to_string()))?;
    scenes::verify(options.scenario, &grid.text())?;
    crate::window::save_image(grid, &host, &options.output)?;
    let mut ansi = Vec::new();
    Host::ansi(&mut ansi, &Grid::new(0, 0), grid, ColorDepth::Truecolor)
        .map_err(|error| Error::Tool(error.to_string()))?;
    write(
        &options.output.with_extension("ansi"),
        ansi,
        Durability::Renamed,
    )?;
    write(
        &options.output.with_extension("txt"),
        grid.text(),
        Durability::Renamed,
    )?;
    eprintln!(
        "UI capture: {} (+ .ansi, .txt{})",
        options.output.display(),
        if crate::window::CAPTURES_IMAGE {
            ", .png"
        } else {
            ""
        }
    );
    Ok(())
}

fn paint(
    screen: &mut Screen,
    host: &mut Host,
    size: (u16, u16),
    output: &mut Vec<u8>,
) -> Result<()> {
    {
        let _phase = profile::span(profile::Phase::TuiPaint);
        screen
            .frame(host, [], size)
            .map_err(|error| Error::Tool(error.to_string()))?;
    }
    let _phase = profile::span(profile::Phase::TuiWrite);
    screen
        .flush(output)
        .map_err(|error| Error::Tool(error.to_string()))?;
    Ok(())
}

fn update(app: &App, options: &Options, index: u16) -> (u16, u16) {
    let _phase = profile::span(profile::Phase::AppUpdate);
    let width = if options.scenario == Scenario::Resize && index.is_multiple_of(2) {
        options.cols - 16
    } else {
        options.cols
    };
    if options.scenario == Scenario::Resize {
        app.resize(width, options.rows);
    } else if options.scenario == Scenario::Stream {
        app.update(Msg::Fact(Fact::Event(Event::Delta {
            text: format!(" token{index}"),
        })));
        app.refresh();
    }
    (width, options.rows)
}

fn counts(options: &Options, size: (u16, u16), bytes: usize) {
    for (name, value) in [
        (profile::Counter::TuiFrames, 1),
        (profile::Counter::TuiBytes, bytes as u64),
        (profile::Counter::WorkloadCols, u64::from(size.0)),
        (profile::Counter::WorkloadRows, u64::from(size.1)),
        (profile::Counter::WorkloadItems, u64::from(options.history)),
        (profile::Counter::WorkloadHeadless, 1),
        (profile::Counter::WorkloadFps, u64::from(options.fps)),
        (profile::Counter::WorkloadScenario, options.scenario as u64),
        (
            profile::Counter::BuildProfile,
            if cfg!(debug_assertions) { 1 } else { 2 },
        ),
    ] {
        profile::count_add(name, value);
    }
}

pub(super) fn info() -> Info {
    Info {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        model: String::from("profile"),
        relay: None,
        session: String::from("profile"),
        endpoint: String::from("profile"),
        pick: None,
        prefs: Prefs {
            animations: false,
            splash: false,
            ..Prefs::default()
        },
        splash: false,
        inline: false,
        strict: false,
        workspace: String::from("profile"),
        workflow: None,
        config: None,
        prompt: None,
        trust: true,
    }
}

fn frame_budget(fps: u32) -> std::time::Duration {
    summonr_agent::limits::limits().frame_window() / fps.clamp(FPS_MIN, FPS_MAX)
}

#[path = "profile_scenes.rs"]
mod scenes;

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
