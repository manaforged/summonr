use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use sigil::Host;
use sigil::app::FrameHooks;
use sigil::events::{Event, KeyPhase, LogicalKey, ModifiersState};
use sigil::prelude::HostMode;
use sigil::prelude::Key;
use sigil::prelude::Role;
use sigil::scene::SceneCommand;
use tokio::sync::mpsc::unbounded_channel;

use super::super::terminal::Term;
use super::super::{App, root};
use super::VIEW;
use summonr_agent::hub::{Cmd, Fact, Msg, Snapshot};
use summonr_agent::provider::Effort;

pub(super) struct Seen<T> {
    pub(super) read: T,
    pub(super) sent: Vec<Cmd>,
    pub(super) controls: Vec<(Role, String, bool)>,
    pub(super) texts: Vec<String>,
}

impl<T> Seen<T> {
    pub(super) fn text(&self) -> String {
        self.texts.join(" | ")
    }

    pub(super) fn named(&self, role: Role, label: &str) -> Vec<bool> {
        self.controls
            .iter()
            .filter(|(at, name, _)| *at == role && name == label)
            .map(|(_, _, disabled)| *disabled)
            .collect()
    }

    pub(super) fn shows(&self, wanted: impl Fn(&str) -> bool) -> bool {
        self.texts.iter().any(|text| wanted(text))
    }
}

pub(super) fn press(key: LogicalKey, modifiers: ModifiersState, text: Option<&str>) -> Event {
    Event::key(key, KeyPhase::Down)
        .modifiers(modifiers)
        .text(text.map(Into::into))
}

pub(super) fn named(key: Key, modifiers: ModifiersState) -> Event {
    press(LogicalKey::named(key), modifiers, None)
}

pub(super) fn typed(text: &str) -> Vec<Event> {
    text.chars()
        .map(|ch| {
            let key = ch.to_string();
            press(
                LogicalKey::character(&key),
                ModifiersState::empty(),
                Some(&key),
            )
        })
        .collect()
}

pub(super) fn command(letter: &str) -> Event {
    press(LogicalKey::character(letter), ModifiersState::META, None)
}

/// One user action in a window check.
pub(super) enum Step {
    Key(Event),
    Click(&'static str),
    /// Clicks the rightmost control with this label, such as the last pane's.
    ClickRightmost(&'static str),
    RightClick(&'static str),
    Wait(Box<dyn FnOnce(&App)>),
    Drag((f32, f32), (f32, f32)),
    /// Drags the control with this label by an offset in pixels.
    DragControl(&'static str, (f32, f32)),
}

pub(super) fn drive<T>(
    setup: impl FnOnce(&App),
    keys: Vec<Event>,
    read: impl FnOnce(&App) -> T,
) -> Seen<T> {
    steps(setup, keys.into_iter().map(Step::Key).collect(), read)
}

fn center(host: &mut Host, label: &str) -> (f32, f32) {
    let bounds = host
        .accessibility_tree()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.label() == Some(label))
                .then(|| node.bounds())
                .flatten()
        })
        .unwrap_or_else(|| panic!("no control named {label}"));
    (
        ((bounds.x0 + bounds.x1) / 2.0) as f32,
        ((bounds.y0 + bounds.y1) / 2.0) as f32,
    )
}

fn click(host: &mut Host, label: &str) {
    let bounds = host
        .accessibility_tree()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.label() == Some(label))
                .then(|| node.bounds())
                .flatten()
        })
        .unwrap_or_else(|| panic!("no control named {label}"));
    press_at(
        host,
        (bounds.x0 + bounds.x1) / 2.0,
        (bounds.y0 + bounds.y1) / 2.0,
    );
}

fn click_rightmost(host: &mut Host, label: &str) {
    let bounds = host
        .accessibility_tree()
        .nodes
        .iter()
        .filter(|(_, node)| node.label() == Some(label))
        .filter_map(|(_, node)| node.bounds())
        .max_by(|a, b| a.x0.total_cmp(&b.x0))
        .unwrap_or_else(|| panic!("no control named {label}"));
    press_at(
        host,
        (bounds.x0 + bounds.x1) / 2.0,
        (bounds.y0 + bounds.y1) / 2.0,
    );
}

fn press_at(host: &mut Host, x: f64, y: f64) {
    let at = sigil::geometry::Point::new(x as f32, y as f32);
    host.route(Event::pointer_down(at));
    host.route(Event::pointer_up(at));
}

fn right_click(host: &mut Host, label: &str) {
    use sigil::events::{MouseButton, MouseButtons, PointerPhase};
    let bounds = host
        .accessibility_tree()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.label() == Some(label))
                .then(|| node.bounds())
                .flatten()
        })
        .unwrap_or_else(|| panic!("no control named {label}"));
    let at = sigil::geometry::Point::new(
        ((bounds.x0 + bounds.x1) / 2.0) as f32,
        ((bounds.y0 + bounds.y1) / 2.0) as f32,
    );
    let press = |phase, buttons| {
        Event::pointer(phase, at)
            .button(Some(MouseButton::Right))
            .buttons(buttons)
    };
    host.route(press(PointerPhase::Down, MouseButtons::RIGHT));
    host.route(press(PointerPhase::Up, MouseButtons::EMPTY));
}

fn drag(host: &mut Host, from: (f32, f32), to: (f32, f32)) {
    let point = |(x, y): (f32, f32)| sigil::geometry::Point::new(x, y);
    host.route(Event::pointer_down(point(from)));
    host.frame(VIEW, 1.0, FrameHooks::default());
    host.route(Event::pointer_move(point(to)));
    host.frame(VIEW, 1.0, FrameHooks::default());
    host.route(Event::pointer_up(point(to)));
}

pub(super) fn steps<T>(
    setup: impl FnOnce(&App),
    steps: Vec<Step>,
    read: impl FnOnce(&App) -> T,
) -> Seen<T> {
    sigil::reactive::Owner::scope("summonr-window", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut receiver) = unbounded_channel();
        let app = App::new(
            crate::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        super::fit(&app);
        setup(&app);
        app.refresh();
        let mut host = Host::new(HostMode::Pixel);
        let tree = app.clone();
        host.mount(move || root(tree.clone()));
        for _ in 0..3 {
            host.frame(VIEW, 1.0, FrameHooks::default());
        }
        for step in steps {
            match step {
                Step::Key(key) => {
                    host.route(key);
                }
                Step::Click(label) => click(&mut host, label),
                Step::ClickRightmost(label) => click_rightmost(&mut host, label),
                Step::RightClick(label) => right_click(&mut host, label),
                Step::Wait(until) => until(&app),
                Step::Drag(from, to) => drag(&mut host, from, to),
                Step::DragControl(label, (dx, dy)) => {
                    let (x, y) = center(&mut host, label);
                    drag(&mut host, (x, y), (x + dx, y + dy));
                }
            }
            host.frame(VIEW, 1.0, FrameHooks::default());
        }
        let read = read(&app);
        host.frame(VIEW, 1.0, FrameHooks::default());
        let mut sent = Vec::new();
        while let Ok(cmd) = receiver.try_recv() {
            sent.push(cmd);
        }
        let controls = host
            .accessibility_tree()
            .nodes
            .iter()
            .filter_map(|(_, node)| {
                node.label().map(|label| {
                    (
                        Role::from_accesskit(node.role()),
                        label.to_string(),
                        node.is_disabled(),
                    )
                })
            })
            .collect();
        let texts = host
            .scene(VIEW)
            .expect("scene")
            .commands
            .into_iter()
            .filter_map(|command| {
                let SceneCommand::GlyphRun(draw) = command else {
                    return None;
                };
                Some(draw.text().to_owned())
            })
            .collect();
        Seen {
            read,
            sent,
            controls,
            texts,
        }
    })
}

pub(super) fn settings(snapshot: Snapshot) -> impl FnOnce(&App) {
    move |app| app.update(Msg::Fact(Fact::Settings(snapshot.into())))
}

pub(super) fn graded() -> Snapshot {
    Snapshot {
        model: "stub-model".to_owned(),
        effort: Some(Effort::Low),
        levels: vec![Effort::Low, Effort::High],
        ..Snapshot::default()
    }
}

pub(super) fn unconfigured() -> Snapshot {
    Snapshot {
        unconfigured: true,
        ..Snapshot::default()
    }
}

pub(super) fn changed(base: &str, files: &[(&str, usize, usize)]) -> summonr_agent::diff::Diff {
    summonr_agent::diff::Diff {
        base: base.to_owned(),
        session: None,
        files: files
            .iter()
            .map(|(path, added, removed)| summonr_agent::diff::Stat {
                path: (*path).to_owned(),
                added: *added as _,
                removed: *removed as _,
                new: false,
            })
            .collect(),
        strays: Vec::new(),
    }
}

const DEADLINE: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(20);

pub(super) type Pump = &'static RefCell<sigil::tasks::AsyncRuntime>;

pub(super) fn pumped() -> Pump {
    let mut runtime = sigil::tasks::AsyncRuntime::new().expect("runtime");
    let waiter = std::thread::current();
    runtime.set_waker(move || waiter.unpark());
    runtime.install_current();
    Box::leak(Box::new(RefCell::new(runtime)))
}

pub(super) fn until(pump: Pump, wanted: impl FnMut() -> bool) -> bool {
    pump_until(pump, DEADLINE, wanted)
}

pub(super) fn pump_for(pump: Pump, span: Duration) {
    pump_until(pump, span, || false);
}

fn pump_until(pump: Pump, span: Duration, mut wanted: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + span;
    loop {
        pump.borrow_mut().pump();
        if wanted() {
            return true;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        std::thread::park_timeout(POLL.min(left));
    }
}

#[derive(Clone, Copy)]
pub(super) enum Which {
    First,
    Last,
}

pub(super) fn screen(app: &App, which: Which) -> Vec<String> {
    let list = app.window.terminals.list.peek();
    let term: Option<&Rc<Term>> = match which {
        Which::First => list.first(),
        Which::Last => list.last(),
    };
    term.map(|term| term.session.lines.get())
        .unwrap_or_default()
}

pub(super) fn screen_until(
    pump: Pump,
    app: &App,
    which: Which,
    wanted: impl Fn(&[String]) -> bool,
) -> Vec<String> {
    let mut seen = Vec::new();
    until(pump, || {
        seen = screen(app, which);
        wanted(&seen)
    });
    seen
}

pub(super) fn has_line(lines: &[String], text: &str) -> bool {
    lines.iter().any(|line| line.trim() == text)
}
