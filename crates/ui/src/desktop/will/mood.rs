use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use sigil::prelude::{Signal, signal};
use sigil::reactive::{effect, untrack};
use summonr_agent::prefs::Reaction;

use super::super::App;
use super::shape::Eyes;
use crate::model::Entry;

/// What Will shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::desktop) enum Mood {
    Idle,
    Working,
    Listening,
    Hop,
    Summoned,
    Asking,
    Pleased,
    Drooping,
    Flinching,
    Noticed,
}

/// How Will holds himself in one mood, on the 16 grid.
pub(super) struct Pose {
    pub(super) scale: (f32, f32),
    pub(super) tilt: f32,
    pub(super) lift: f32,
    pub(super) fade: f32,
    pub(super) flicker: f32,
    pub(super) sway: f32,
    pub(super) stretch: f32,
    pub(super) eyes: Eyes,
    pub(super) look: (f32, f32),
}

const REST: Pose = Pose {
    scale: (1.0, 1.0),
    tilt: 0.0,
    lift: 0.0,
    fade: 1.0,
    flicker: 2.6,
    sway: 2.0,
    stretch: 0.02,
    eyes: Eyes::Open,
    look: (0.0, 0.0),
};

impl Mood {
    /// The pose for this mood.
    pub(super) const fn pose(self) -> Pose {
        match self {
            Self::Idle => REST,
            Self::Working => Pose {
                scale: (1.03, 1.08),
                flicker: 1.1,
                sway: 2.1,
                stretch: 0.025,
                eyes: Eyes::Focus,
                ..REST
            },
            Self::Listening => Pose {
                tilt: 5.0,
                look: (0.7, 0.5),
                ..REST
            },
            Self::Hop => Pose {
                scale: (1.06, 1.14),
                lift: -1.6,
                eyes: Eyes::Wide,
                ..REST
            },
            Self::Summoned => Pose {
                scale: (0.55, 0.55),
                fade: 0.4,
                eyes: Eyes::Wide,
                ..REST
            },
            Self::Asking => Pose {
                tilt: -6.0,
                eyes: Eyes::Wide,
                ..REST
            },
            Self::Pleased => Pose {
                scale: (1.06, 1.12),
                eyes: Eyes::Happy,
                ..REST
            },
            Self::Drooping => Pose {
                scale: (1.12, 0.72),
                fade: 0.6,
                flicker: 3.6,
                eyes: Eyes::Sad,
                ..REST
            },
            Self::Flinching => Pose {
                scale: (1.06, 0.88),
                eyes: Eyes::Closed,
                ..REST
            },
            Self::Noticed => Pose {
                scale: (1.08, 1.08),
                eyes: Eyes::Wide,
                ..REST
            },
        }
    }
}

/// A short reaction: the mood it shows, how it ranks against others, and how long it holds.
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

const STOP: Flash = flash(Mood::Flinching, 80, 900, Some(Reaction::Stop));
const SEND: Flash = flash(Mood::Hop, 70, 450, Some(Reaction::Send));
const SUMMON: Flash = flash(Mood::Summoned, 68, 260, None);
const SWITCH: Flash = flash(Mood::Noticed, 65, 500, None);
const FAIL: Flash = flash(Mood::Drooping, 60, 3000, Some(Reaction::Fail));
const FINISH: Flash = flash(Mood::Pleased, 60, 2500, Some(Reaction::Finish));
const TYPE: Flash = flash(Mood::Listening, 40, 1200, Some(Reaction::Type));

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
        | Entry::Image { .. } => FINISH,
    }
}

const HOVER_RANK: u8 = 90;
const ASK_RANK: u8 = 50;
const WORK_RANK: u8 = 30;

/// What the agent was doing at the last look.
struct Seen {
    session: String,
    busy: bool,
    typed: usize,
}

/// Will's reactions to one agent pane: short flashes from what just happened, plus hover.
#[derive(Clone)]
pub(super) struct Mind {
    flashes: Rc<RefCell<Vec<(Flash, Instant)>>>,
    pub(super) hover: Signal<bool>,
    pub(super) version: Signal<u64>,
    pub(super) typed_at: Rc<RefCell<Option<Instant>>>,
}

impl Mind {
    /// Watches `app` and raises a flash when the user sends, stops, types or switches agents, and when a turn ends.
    pub(super) fn watch(app: &App) -> Self {
        let mind = Self {
            flashes: Rc::default(),
            hover: signal(false),
            version: signal(0),
            typed_at: Rc::default(),
        };
        let (model, draft, raise) = (app.model, app.draft, mind.clone());
        let seen = RefCell::new(None::<Seen>);
        effect(move || {
            let (session, busy, empty, ended) = model.with(|model| {
                let ended = model.entries.last().map(ending);
                (
                    model.session.clone(),
                    model.busy_since.is_some(),
                    model.entries.len() == 0,
                    ended,
                )
            });
            let typed = draft.with(String::len);
            untrack(|| {
                let mut seen = seen.borrow_mut();
                if let Some(was) = seen.as_ref() {
                    if was.session != session {
                        raise.raise(if empty { SUMMON } else { SWITCH });
                    } else if !was.busy && busy {
                        raise.raise(SEND);
                    } else if was.busy && !busy {
                        raise.raise(ended.unwrap_or(FINISH));
                    }
                    if typed > was.typed && was.session == session {
                        raise.raise(TYPE);
                        *raise.typed_at.borrow_mut() = Some(Instant::now());
                    }
                } else if empty {
                    raise.raise(SUMMON);
                }
                *seen = Some(Seen {
                    session,
                    busy,
                    typed,
                });
            });
        });
        mind
    }

    fn raise(&self, flash: Flash) {
        let mut flashes = self.flashes.borrow_mut();
        flashes.retain(|(each, _)| each.mood != flash.mood);
        flashes.push((flash, Instant::now() + flash.hold));
        drop(flashes);
        self.version.update(|at| *at = at.wrapping_add(1));
    }

    /// The mood that wins now; tracks the app signals it reads.
    pub(super) fn mood(&self, app: &App) -> Mood {
        let _ = self.version.get();
        let quiet = app.model.with(|model| model.prefs.will_quiet.clone());
        let working = app.model.with(|model| model.busy_since.is_some());
        let asking = app.approval.with(Option::is_some) || app.form.with(Option::is_some);
        let now = Instant::now();
        let mut best = (0, Mood::Idle);
        let mut offer = |rank: u8, mood: Mood| {
            if rank > best.0 {
                best = (rank, mood);
            }
        };
        if self.hover.get() {
            offer(HOVER_RANK, Mood::Noticed);
        }
        for (flash, until) in self.flashes.borrow().iter() {
            let allowed = flash.reaction.is_none_or(|each| !quiet.contains(&each));
            if *until > now && allowed {
                offer(flash.rank, flash.mood);
            }
        }
        if asking && !quiet.contains(&Reaction::Ask) {
            offer(ASK_RANK, Mood::Asking);
        }
        if working {
            offer(WORK_RANK, Mood::Working);
        }
        best.1
    }
}
