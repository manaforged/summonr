use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use sigil::prelude::{Signal, signal};
use sigil::reactive::{effect, untrack};
use summonr_agent::prefs::Reaction;

use super::super::App;
use crate::model::Entry;

/// What Will shows: the states on the design sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::desktop) enum Mood {
    Idle,
    Summoned,
    Notice,
    Typing,
    Thinking,
    Working,
    Needs,
    Passed,
    Failed,
    Stopped,
    Doze,
}

/// A one-shot clip that plays over any mood.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shot {
    Hop,
    Flinch,
    Burst,
    Sweep,
}

impl Shot {
    /// How long the clip runs.
    pub(super) const fn length(self) -> Duration {
        Duration::from_millis(match self {
            Self::Hop => 600,
            Self::Flinch => 400,
            Self::Burst => 800,
            Self::Sweep => 1800,
        })
    }
}

/// A held mood: how it ranks against others, how long it holds, and the switch that turns it off.
#[derive(Clone, Copy)]
struct Flash {
    mood: Mood,
    rank: u8,
    hold: Duration,
    reaction: Option<Reaction>,
}

const fn flash(mood: Mood, rank: u8, millis: u64, reaction: Option<Reaction>) -> Flash {
    Flash {
        mood,
        rank,
        hold: Duration::from_millis(millis),
        reaction,
    }
}

const SUMMON: Flash = flash(Mood::Summoned, 95, 900, None);
const PASS: Flash = flash(Mood::Passed, 80, 2600, Some(Reaction::Finish));
const FAIL: Flash = flash(Mood::Failed, 80, 2600, Some(Reaction::Fail));
const STOP: Flash = flash(Mood::Stopped, 80, 2300, Some(Reaction::Stop));
const NOTICE: Flash = flash(Mood::Notice, 50, 1100, Some(Reaction::Hover));
const SWITCH: Flash = flash(Mood::Notice, 50, 900, None);
const TYPE: Flash = flash(Mood::Typing, 40, 1400, Some(Reaction::Type));

const NEEDS_RANK: u8 = 70;
const BUSY_RANK: u8 = 60;
const DOZE_RANK: u8 = 10;
/// How long Will waits alone before he dozes.
const ALONE: Duration = Duration::from_secs(12);
/// The shortest gap between two reactions to the pointer.
const CALMER: Duration = Duration::from_secs(2);
/// How far the eyes follow the pointer, across and down, on the 120 grid.
const FOLLOW: (f32, f32) = (4.0, 3.0);

const fn ending(entry: &Entry) -> Flash {
    match entry {
        Entry::Aborted(_) => STOP,
        Entry::Error(_) => FAIL,
        Entry::User(_)
        | Entry::Thinking(_)
        | Entry::Receipt(_)
        | Entry::Assistant { .. }
        | Entry::Tool { .. }
        | Entry::Verify { .. }
        | Entry::Mail { .. }
        | Entry::System(_)
        | Entry::Injected(_)
        | Entry::Chart(_)
        | Entry::Image { .. } => PASS,
    }
}

const fn waiting(entry: &Entry) -> bool {
    matches!(entry, Entry::User(_) | Entry::Thinking(_))
}

/// What the agent was doing at the last look.
struct Seen {
    session: String,
    busy: bool,
    count: usize,
    typed: usize,
}

/// Where Will is on screen: the centre of his body, and his size, in window points.
pub(super) type Spot = Rc<dyn Fn() -> (f32, f32, f32)>;

/// Will's reactions to one agent pane: held moods, one-shot clips, and where the pointer is.
#[derive(Clone)]
pub(super) struct Mind {
    flashes: Rc<RefCell<Vec<(Flash, Instant)>>>,
    shots: Rc<RefCell<Vec<(Shot, Instant)>>>,
    active: Rc<Cell<Instant>>,
    reacted: Rc<Cell<Option<Instant>>>,
    version: Signal<u64>,
    pointer: Rc<Cell<Option<(f32, f32)>>>,
    spot: Spot,
}

impl Mind {
    /// Watches `app` for sends, turn ends, typing and agent switches.
    pub(super) fn watch(app: &App, pointer: Rc<Cell<Option<(f32, f32)>>>, spot: Spot) -> Self {
        let mind = Self {
            flashes: Rc::default(),
            shots: Rc::default(),
            active: Rc::new(Cell::new(Instant::now())),
            reacted: Rc::default(),
            version: signal(0),
            pointer,
            spot,
        };
        let (model, draft, raise, watched) = (app.model, app.draft, mind.clone(), app.clone());
        let seen = RefCell::new(None::<Seen>);
        effect(move || {
            let (session, busy, count, ended) = model.with(|model| {
                (
                    model.session.clone(),
                    model.busy_since.is_some(),
                    model.entries.len(),
                    model.entries.last().map(ending),
                )
            });
            let typed = draft.with(String::len);
            untrack(|| {
                let mut seen = seen.borrow_mut();
                let now = Seen {
                    session,
                    busy,
                    count,
                    typed,
                };
                raise.notice(&watched, seen.as_ref(), &now, ended);
                *seen = Some(now);
            });
        });
        mind
    }

    fn notice(&self, app: &App, was: Option<&Seen>, now: &Seen, ended: Option<Flash>) {
        let Some(was) = was else {
            if now.count == 0 {
                self.raise(SUMMON);
            }
            return;
        };
        if was.session != now.session {
            self.raise(if now.count == 0 { SUMMON } else { SWITCH });
        } else if !was.busy && now.busy {
            if allowed(app, Reaction::Send) {
                self.play(Shot::Hop);
            }
        } else if was.busy && !now.busy {
            self.raise(ended.unwrap_or(PASS));
        }
        if now.typed > was.typed && was.session == now.session && !now.busy {
            self.raise(TYPE);
        }
        if was.busy != now.busy || was.count != now.count || was.typed != now.typed {
            self.active.set(Instant::now());
        }
    }

    /// The pointer came onto Will: he notices it.
    pub(super) fn enter(&self, app: &App) {
        self.active.set(Instant::now());
        let busy = untrack(|| app.model.with(|model| model.busy_since.is_some()));
        if !busy && allowed(app, Reaction::Hover) && self.calm() {
            self.raise(NOTICE);
        }
    }

    /// The user clicked Will without dragging him: a hop and a spark burst.
    pub(super) fn poke(&self, app: &App) {
        self.active.set(Instant::now());
        if allowed(app, Reaction::Poke) && self.calm() {
            self.play(Shot::Hop);
            self.play(Shot::Burst);
        }
    }

    /// Whether the last reaction to the pointer was long enough ago; marks a new one if so.
    fn calm(&self) -> bool {
        let now = Instant::now();
        if self
            .reacted
            .get()
            .is_some_and(|at| now.duration_since(at) < CALMER)
        {
            return false;
        }
        self.reacted.set(Some(now));
        true
    }

    fn raise(&self, flash: Flash) {
        let mut flashes = self.flashes.borrow_mut();
        flashes.retain(|(each, until)| each.mood != flash.mood && *until > Instant::now());
        flashes.push((flash, Instant::now() + flash.hold));
        drop(flashes);
        self.active.set(Instant::now());
        self.version.update(|at| *at = at.wrapping_add(1));
    }

    fn play(&self, shot: Shot) {
        let mut shots = self.shots.borrow_mut();
        shots.retain(|(each, at)| *each != shot && at.elapsed() < each.length());
        shots.push((shot, Instant::now()));
    }

    /// The clips playing now.
    pub(super) fn shots(&self) -> Vec<(Shot, Instant)> {
        self.shots.borrow().clone()
    }

    /// Where the eyes look to follow the pointer, on the 120 grid.
    pub(super) fn look(&self, app: &App) -> (f32, f32) {
        let Some((x, y)) = self.pointer.get() else {
            return (0.0, 0.0);
        };
        if !allowed(app, Reaction::Look) {
            return (0.0, 0.0);
        }
        let (cx, cy, size) = (self.spot)();
        let across = (x - cx) / (size * 1.5) * 9.0;
        let down = (y - cy) / (size * 1.33) * 7.0;
        (
            across.clamp(-FOLLOW.0, FOLLOW.0),
            down.clamp(-FOLLOW.1, FOLLOW.1),
        )
    }

    /// The mood that wins now; tracks the app signals it reads.
    pub(super) fn mood(&self, app: &App) -> Mood {
        let _ = self.version.get();
        let quiet = app.model.with(|model| model.prefs.will_quiet.clone());
        let (busy, waiting) = app.model.with(|model| {
            (
                model.busy_since.is_some(),
                model.entries.last().is_some_and(waiting),
            )
        });
        let asking = app.approval.with(Option::is_some) || app.form.with(Option::is_some);
        let now = Instant::now();
        let mut best = (0, Mood::Idle);
        let mut offer = |rank: u8, mood: Mood| {
            if rank > best.0 {
                best = (rank, mood);
            }
        };
        for (flash, until) in self.flashes.borrow().iter() {
            let allowed = flash.reaction.is_none_or(|each| !quiet.contains(&each));
            if *until > now && allowed {
                offer(flash.rank, flash.mood);
            }
        }
        if asking && !quiet.contains(&Reaction::Ask) {
            offer(NEEDS_RANK, Mood::Needs);
        }
        if busy {
            offer(
                BUSY_RANK,
                if waiting {
                    Mood::Thinking
                } else {
                    Mood::Working
                },
            );
        }
        let alone = now.duration_since(self.active.get()) >= ALONE;
        if alone && !busy && !asking && !quiet.contains(&Reaction::Doze) {
            offer(DOZE_RANK, Mood::Doze);
        }
        best.1
    }
}

fn allowed(app: &App, reaction: Reaction) -> bool {
    untrack(|| {
        app.model
            .with(|model| !model.prefs.will_quiet.contains(&reaction))
    })
}
